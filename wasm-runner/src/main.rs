use wasmedge_sys::{
    WasiModule,
    Config,
    Store,
    Executor,
    Loader,
    Validator,
    AsInstance,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::{Result, anyhow};
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use nix::unistd::{dup, dup2, close};
use clap::Parser;
use os_pipe::pipe;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    wasm: Option<String>,

    #[arg(short, long)]
    json: bool,

    #[arg(short, long)]
    preopen: Vec<String>, // format "guest:host"

    #[arg(last = true)]
    guest_args: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RunRequest {
    wasm_path: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    preopens: Vec<(String, String)>,
    cwd: Option<String>,
}

#[derive(Debug, Serialize)]
struct RunResponse {
    stdout: String,
    stderr: String,
    exit_code: i32,
    ok: bool,
    error: Option<String>,
}

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::Mutex as AsyncMutex;
use std::sync::LazyLock;

static STDIO_LOCK: LazyLock<AsyncMutex<()>> = LazyLock::new(|| AsyncMutex::new(()));

#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = Args::parse();

    if cli_args.json {
        let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
        let mut stdout = tokio::io::stdout();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() { continue; }

            let request: RunRequest = match serde_json::from_str(&line) {
                Ok(req) => req,
                Err(e) => {
                    let resp = RunResponse {
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 1,
                        ok: false,
                        error: Some(format!("Failed to parse request: {}", e)),
                    };
                    let json = serde_json::to_vec(&resp)?;
                    stdout.write_all(&json).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                    continue;
                }
            };

            match run_wasm_json(request).await {
                Ok(resp) => {
                    if let Ok(json) = serde_json::to_vec(&resp) {
                        let _ = stdout.write_all(&json).await;
                        let _ = stdout.write_all(b"\n").await;
                        let _ = stdout.flush().await;
                    }
                },
                Err(e) => {
                    let resp = RunResponse {
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 1,
                        ok: false,
                        error: Some(format!("Execution failed: {}", e)),
                    };
                    if let Ok(json) = serde_json::to_vec(&resp) {
                        let _ = stdout.write_all(&json).await;
                        let _ = stdout.write_all(b"\n").await;
                        let _ = stdout.flush().await;
                    }
                }
            }
        }
    } else {
        // Direct CLI mode (Intercepted from command-daemon)
        let wasm_path = cli_args.wasm.ok_or_else(|| anyhow!("Missing --wasm"))?;
        
        let mut preopens = Vec::new();
        for p in cli_args.preopen {
            let parts: Vec<&str> = p.split(':').collect();
            if parts.len() == 2 {
                preopens.push((parts[0].to_string(), parts[1].to_string()));
            } else {
                preopens.push((p.clone(), p));
            }
        }
        
        /* [AUDIT FIX]: Always map host CWD to guest "." to ensure relative path transparency */
        if !preopens.iter().any(|(g, _)| g == ".") {
            preopens.push((".".to_string(), ".".to_string()));
        }

        let _lock = STDIO_LOCK.lock().await;
        let result = run_wasm_cli(&wasm_path, &cli_args.guest_args, preopens).await;
        
        match result {
            Ok(exit_code) => std::process::exit(exit_code),
            Err(e) => {
                eprintln!("[wasm-runner error] {}", e);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

async fn run_wasm_cli(wasm_path: &str, guest_args: &[String], preopens: Vec<(String, String)>) -> Result<i32> {
    let (out_reader, out_writer) = pipe()?;
    let (err_reader, err_writer) = pipe()?;

    let stdin_orig = dup(0).map_err(|e| anyhow!("Failed to dup stdin: {}", e))?;
    let stdout_orig = dup(1).map_err(|e| anyhow!("Failed to dup stdout: {}", e))?;
    let stderr_orig = dup(2).map_err(|e| anyhow!("Failed to dup stderr: {}", e))?;
    
    let _stdio_guard = StdioGuard { stdin_orig, stdout_orig, stderr_orig };

    let dev_null = std::fs::File::open("/dev/null").map_err(|e| anyhow!("Failed to open /dev/null: {}", e))?;
    dup2(dev_null.as_raw_fd(), 0).map_err(|e| anyhow!("Failed to redirect stdin: {}", e))?;
    dup2(out_writer.as_raw_fd(), 1).map_err(|e| anyhow!("Failed to redirect stdout: {}", e))?;
    dup2(err_writer.as_raw_fd(), 2).map_err(|e| anyhow!("Failed to redirect stderr: {}", e))?;
    let mut wasi_args = vec!["python.wasm".to_string()];
    wasi_args.extend(guest_args.iter().cloned());
    
    let envs = [
        "PYTHONHOME=/".to_string(),
        "PYTHONPATH=/lib/python3.11".to_string(),
    ];
    
    let preopen_strs: Vec<String> = preopens.into_iter().map(|(g, h)| format!("{}:{}", g, h)).collect();

    use std::fs::File;
    use std::os::unix::io::FromRawFd;

    let stdout_task_fd = dup(stdout_orig).map_err(|e| anyhow!("Failed to dup stdout for task: {}", e))?;
    let stderr_task_fd = dup(stderr_orig).map_err(|e| anyhow!("Failed to dup stderr for task: {}", e))?;

    let out_read_handle = tokio::task::spawn_blocking(move || {
        let mut buf = unsafe { File::from_raw_fd(stdout_task_fd) };
        let mut reader = out_reader;
        let _ = io::copy(&mut reader, &mut buf);
    });
    
    let err_read_handle = tokio::task::spawn_blocking(move || {
        let mut buf = unsafe { File::from_raw_fd(stderr_task_fd) };
        let mut reader = err_reader;
        let _ = io::copy(&mut reader, &mut buf);
    });

    let exec_result = {
        let wasi_module = WasiModule::create(
            Some(wasi_args.iter().map(|s| s.as_str()).collect()),
            Some(envs.iter().map(|s| s.as_str()).collect()),
            Some(preopen_strs.iter().map(|s| s.as_str()).collect()),
        ).map_err(|e| anyhow!("Failed to create WASI module: {:?}", e))?;

        let mut config = Config::create().map_err(|e| anyhow!("Failed to create config: {:?}", e))?;
        config.set_max_memory_pages(4096);
        let loader = Loader::create(Some(&config)).map_err(|e| anyhow!("Failed to create loader: {:?}", e))?;
        let validator = Validator::create(Some(&config)).map_err(|e| anyhow!("Failed to create validator: {:?}", e))?;
        let mut executor = Executor::create(Some(&config), None).map_err(|e| anyhow!("Failed to create executor: {:?}", e))?;
        let mut store = Store::create().map_err(|e| anyhow!("Failed to create store: {:?}", e))?;

        let base_wasm_path = PathBuf::from(wasm_path);
        let aot_path = base_wasm_path.with_extension("wasm.aot");
        let wasm_to_load = if aot_path.exists() { aot_path } else { base_wasm_path };
        let module = loader.from_file(&wasm_to_load).map_err(|e| anyhow!("Failed to load wasm: {:?}", e))?;
        validator.validate(&module).map_err(|e| anyhow!("Failed to validate wasm: {:?}", e))?;
        
        executor.register_import_module(&mut store, &wasi_module).map_err(|e| anyhow!("Failed to register WASI: {:?}", e))?;
            
        let mut active_instance = executor.register_active_module(&mut store, &module)
            .map_err(|e| anyhow!("Failed to register active module: {:?}", e))?;

        let handle = tokio::task::spawn_blocking(move || -> Result<()> {
            if let Ok(mut func) = active_instance.get_func_mut("_start") {
                 executor.call_func(&mut func, []).map_err(|e| anyhow!("Wasm execution failed: {:?}", e))?;
                 Ok(())
            } else {
                Err(anyhow!("Failed to find _start function"))
            }
        });
        handle.await.unwrap_or_else(|e| Err(anyhow!("Join error: {}", e)))
    };

    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    drop(out_writer);
    drop(err_writer);
    drop(_stdio_guard);

    let _ = out_read_handle.await;
    let _ = err_read_handle.await;

    match exec_result {
        Ok(_) => Ok(0),
        Err(e) => {
            eprintln!("[wasm-runner error] {}", e);
            Ok(1)
        }
    }
}

struct StdioGuard {
    stdin_orig: i32,
    stdout_orig: i32,
    stderr_orig: i32,
}
impl Drop for StdioGuard {
    fn drop(&mut self) {
        let _ = dup2(self.stdin_orig, 0);
        let _ = dup2(self.stdout_orig, 1);
        let _ = dup2(self.stderr_orig, 2);
        let _ = close(self.stdin_orig);
        let _ = close(self.stdout_orig);
        let _ = close(self.stderr_orig);
    }
}

struct CwdGuard {
    original_cwd: Option<PathBuf>,
}
impl Drop for CwdGuard {
    fn drop(&mut self) {
        if let Some(path) = &self.original_cwd {
            let _ = std::env::set_current_dir(path);
        }
    }
}

async fn run_wasm_json(req: RunRequest) -> Result<RunResponse> {
    let _lock = STDIO_LOCK.lock().await;
    let (out_reader, out_writer) = pipe()?;
    let (err_reader, err_writer) = pipe()?;

    let stdin_orig = dup(0).map_err(|e| anyhow!("Failed to dup stdin: {}", e))?;
    let stdout_orig = dup(1).map_err(|e| anyhow!("Failed to dup stdout: {}", e))?;
    let stderr_orig = dup(2).map_err(|e| anyhow!("Failed to dup stderr: {}", e))?;
    
    let _stdio_guard = StdioGuard { stdin_orig, stdout_orig, stderr_orig };

    let dev_null = std::fs::File::open("/dev/null").map_err(|e| anyhow!("Failed to open /dev/null: {}", e))?;
    dup2(dev_null.as_raw_fd(), 0).map_err(|e| anyhow!("Failed to redirect stdin: {}", e))?;
    dup2(out_writer.as_raw_fd(), 1).map_err(|e| anyhow!("Failed to redirect stdout: {}", e))?;
    dup2(err_writer.as_raw_fd(), 2).map_err(|e| anyhow!("Failed to redirect stderr: {}", e))?;
    let mut wasi_args = vec!["python.wasm".to_string()];
    wasi_args.extend(req.args);
    
    let mut envs: Vec<String> = req.env.into_iter().map(|(k, v)| format!("{}={}", k, v)).collect();
    if !envs.iter().any(|e| e.starts_with("PYTHONHOME=")) {
        envs.push("PYTHONHOME=/".to_string());
    }
    if !envs.iter().any(|e| e.starts_with("PYTHONPATH=")) {
        envs.push("PYTHONPATH=/lib/python3.11".to_string());
    }

    let mut preopens: Vec<String> = req.preopens.into_iter().map(|(g, h)| format!("{}:{}", g, h)).collect();
    if !preopens.iter().any(|p| p.starts_with(".:") || p.starts_with("./:")) {
        preopens.push(".:.".to_string());
    }

    // Handle CWD
    let original_cwd = std::env::current_dir().ok();
    if let Some(ref target_cwd) = req.cwd {
        std::env::set_current_dir(target_cwd).map_err(|e| anyhow!("Failed to chdir to {}: {}", target_cwd, e))?;
    }
    let _cwd_guard = CwdGuard { original_cwd };

    // Read from pipes in parallel to WASM execution
    let mut out_reader = out_reader;
    let mut err_reader = err_reader;
    const MAX_OUTPUT: usize = 10 * 1024 * 1024; // 10MB limit

    let out_read_handle = tokio::task::spawn_blocking(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut truncated = false;
        while let Ok(n) = out_reader.read(&mut chunk) {
            if n == 0 { break; }
            if truncated { continue; }
            if buf.len() + n > MAX_OUTPUT {
                let remaining = MAX_OUTPUT - buf.len();
                buf.extend_from_slice(&chunk[..remaining]);
                buf.extend_from_slice(b"\n... [STDOUT TRUNCATED] ...");
                truncated = true;
                continue;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8_lossy(&buf).to_string()
    });
    
    let err_read_handle = tokio::task::spawn_blocking(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut truncated = false;
        while let Ok(n) = err_reader.read(&mut chunk) {
            if n == 0 { break; }
            if truncated { continue; }
            if buf.len() + n > MAX_OUTPUT {
                let remaining = MAX_OUTPUT - buf.len();
                buf.extend_from_slice(&chunk[..remaining]);
                buf.extend_from_slice(b"\n... [STDERR TRUNCATED] ...");
                truncated = true;
                continue;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8_lossy(&buf).to_string()
    });

    // Run WASM execution in a blocking thread.
    let exec_result: Result<()> = {
        let wasi_module = WasiModule::create(
            Some(wasi_args.iter().map(|s| s.as_str()).collect()),
            Some(envs.iter().map(|s| s.as_str()).collect()),
            Some(preopens.iter().map(|s| s.as_str()).collect()),
        ).map_err(|e| anyhow!("Failed to create WASI module: {:?}", e))?;

        let mut config = Config::create().map_err(|e| anyhow!("Failed to create config: {:?}", e))?;
        config.set_max_memory_pages(4096);
        let loader = Loader::create(Some(&config)).map_err(|e| anyhow!("Failed to create loader: {:?}", e))?;
        let validator = Validator::create(Some(&config)).map_err(|e| anyhow!("Failed to create validator: {:?}", e))?;
        let mut executor = Executor::create(Some(&config), None).map_err(|e| anyhow!("Failed to create executor: {:?}", e))?;
        let mut store = Store::create().map_err(|e| anyhow!("Failed to create store: {:?}", e))?;

        let base_wasm_path = PathBuf::from(&req.wasm_path);
        let aot_path = base_wasm_path.with_extension("wasm.aot");
        let wasm_to_load = if aot_path.exists() { aot_path } else { base_wasm_path };
        let module = loader.from_file(&wasm_to_load).map_err(|e| anyhow!("Failed to load wasm: {:?}", e))?;
        validator.validate(&module).map_err(|e| anyhow!("Failed to validate wasm: {:?}", e))?;
        
        executor.register_import_module(&mut store, &wasi_module).map_err(|e| anyhow!("Failed to register WASI: {:?}", e))?;
            
        let mut active_instance = executor.register_active_module(&mut store, &module)
            .map_err(|e| anyhow!("Failed to register active module: {:?}", e))?;

        let handle = tokio::task::spawn_blocking(move || -> Result<()> {
            if let Ok(mut func) = active_instance.get_func_mut("_start") {
                 executor.call_func(&mut func, []).map_err(|e| anyhow!("Wasm execution failed: {:?}", e))?;
                 Ok(())
            } else {
                Err(anyhow!("Failed to find _start function"))
            }
        });
        handle.await.unwrap_or_else(|e| Err(anyhow!("Join error: {}", e)))
    };

    // Ensure writers are dropped so readers get EOF
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    drop(out_writer);
    drop(err_writer);

    // Drop guards to restore stdio and CWD *before* awaiting readers, 
    // so that fd 1 and 2 are closed and readers receive EOF.
    drop(_stdio_guard);
    drop(_cwd_guard);

    // Get output
    let stdout = out_read_handle.await.unwrap_or_default();
    let stderr = err_read_handle.await.unwrap_or_default();

    let (ok, error) = match exec_result {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    };

    Ok(RunResponse {
        stdout,
        stderr,
        exit_code: if ok { 0 } else { 1 },
        ok,
        error,
    })
}

