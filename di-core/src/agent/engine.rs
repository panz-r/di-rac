use crate::agent::trajectory::{Trajectory, Role, Message};
use crate::observer::Observer;
use crate::context::ContextManager;
use crate::daemons::{UnixDaemonClient, HttpDaemonClient, CentralRequest, CentralResponse, ApiGatewayRequest};
use crate::protocol::CoreEvent;
use crate::tools::{ToolExecutor, ToolCall};
use crate::agent::parser::ResponseParser;
use anyhow::{Result, anyhow};
use serde_json::{json, Value};
use std::collections::{HashSet, HashMap};
use std::sync::Arc;
use uuid::Uuid;

pub struct AgentEngine {
    pub id: Uuid,
    pub trajectory: Trajectory,
    pub observer: Observer,
    pub context_manager: ContextManager,
    pub analyzer_client: Arc<UnixDaemonClient>,
    pub command_client: Arc<UnixDaemonClient>,
    pub central_client: Arc<UnixDaemonClient>,
    pub gateway_client: Arc<HttpDaemonClient>,
    pub tool_executor: ToolExecutor,
    pub parser: ResponseParser,
}

impl AgentEngine {
    pub fn new(
        id: Uuid, 
        analyzer_client: Arc<UnixDaemonClient>, 
        command_client: Arc<UnixDaemonClient>,
        central_client: Arc<UnixDaemonClient>,
        gateway_client: Arc<HttpDaemonClient>,
    ) -> Self {
        Self {
            id,
            trajectory: Trajectory::new(),
            observer: Observer::new(),
            context_manager: ContextManager::new(32000, 24000),
            analyzer_client: analyzer_client.clone(),
            command_client: command_client.clone(),
            central_client,
            gateway_client,
            tool_executor: ToolExecutor::new(analyzer_client, command_client),
            parser: ResponseParser::new(),
        }
    }

    /// Execute one turn of the agent loop.
    pub async fn run_turn(&mut self) -> Result<()> {
        // 0. Fetch Config from Central Daemon
        let system_prompt = self.get_config("system_prompt").await?
            .as_str().unwrap_or("You are a helpful assistant.").to_string();
        let model = self.get_config("model").await?
            .as_str().unwrap_or("gpt-4").to_string();

        // 1. Structural Analysis (API Extraction)
        let current_apis = self.extract_current_apis()?;

        // 2. Build Prompt (Memory Pyramid)
        let messages = self.context_manager.build_prompt(&system_prompt, &self.trajectory, &current_apis);
        
        // 3. Observer Pass (System 1)
        let sqs = self.observer.compute_sqs(&self.trajectory);
        
        self.emit_event(CoreEvent::MetricsUpdate {
            agent_id: self.id,
            sqs: sqs.score,
            token_usage: messages.iter().map(|m| m.tokens).sum(),
            latency_ms: 0, 
        })?;

        // 4. Think (LLM Interaction)
        // Convert Trajectory messages to API Gateway format
        let gateway_msgs: Vec<Value> = messages.iter().map(|m| json!({
            "role": match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => "system",
                Role::Tool => "tool",
            },
            "content": m.content
        })).collect();

        // In a real streaming implementation, we'd handle chunks. 
        // For now, let's assume a full response for logic porting.
        let resp: Value = self.gateway_client.post("chat/completions", ApiGatewayRequest {
            model: model.clone(),
            messages: gateway_msgs,
            stream: false,
        }).await?;

        let response_text = resp["choices"][0]["message"]["content"].as_str()
            .ok_or_else(|| anyhow!("Invalid response from api-gateway"))?;

        // 5. Parse Response
        let (thought, tools) = self.parser.parse(response_text);
        
        // Record Assistant's thought
        self.trajectory.add_message(Role::Assistant, json!(thought), thought.len() / 4);
        self.emit_event(CoreEvent::ThoughtFinished { agent_id: self.id })?;

        // 6. Act (Process Tool Calls)
        for tool in tools {
            self.emit_event(CoreEvent::ToolCallStarted { 
                agent_id: self.id, 
                tool: tool.name.clone(), 
                args: tool.args.clone() 
            })?;

            match self.tool_executor.execute(&tool).await {
                Ok(result) => {
                    self.trajectory.add_message(Role::Tool, result.clone(), 100); // TODO: proper token count
                    self.emit_event(CoreEvent::ToolCallFinished { 
                        agent_id: self.id, 
                        result 
                    })?;
                }
                Err(e) => {
                    let error_msg = json!({ "error": e.to_string() });
                    self.trajectory.add_message(Role::Tool, error_msg.clone(), 50);
                    self.emit_event(CoreEvent::ToolCallFinished { 
                        agent_id: self.id, 
                        result: error_msg 
                    })?;
                }
            }
        }

        Ok(())
    }

    async fn get_config(&self, key: &str) -> Result<Value> {
        let resp: CentralResponse = self.central_client.send_request(CentralRequest {
            command: "get".to_string(),
            key: Some(key.to_string()),
            value: None,
        })?;

        if resp.ok {
            Ok(resp.value.unwrap_or(Value::Null))
        } else {
            Err(anyhow!("Failed to fetch config for {}: {:?}", key, resp.error))
        }
    }

    fn extract_current_apis(&self) -> Result<HashSet<String>> {
        let mut apis = HashSet::new();
        if let Some(msg) = self.trajectory.messages.iter().filter(|m| matches!(m.role, Role::Assistant)).last() {
            let content = msg.content.to_string();
            if let Ok(resp) = self.analyzer_client.extract_apis(&content, "python") {
                for call in resp.calls { apis.insert(call); }
                for def in resp.definitions { apis.insert(def); }
            }
        }
        Ok(apis)
    }

    fn emit_event(&self, event: CoreEvent) -> Result<()> {
        println!("{}", serde_json::to_string(&event)?);
        Ok(())
    }
}

pub struct MultiAgentOrchestrator {
    pub agents: HashMap<Uuid, AgentEngine>,
    pub analyzer_client: Arc<UnixDaemonClient>,
    pub command_client: Arc<UnixDaemonClient>,
    pub central_client: Arc<UnixDaemonClient>,
    pub gateway_client: Arc<HttpDaemonClient>,
}

impl MultiAgentOrchestrator {
    pub fn new(analyzer_socket: &str, command_socket: &str, central_socket: &str, gateway_url: &str) -> Self {
        Self {
            agents: HashMap::new(),
            analyzer_client: Arc::new(UnixDaemonClient::new(analyzer_socket)),
            command_client: Arc::new(UnixDaemonClient::new(command_socket)),
            central_client: Arc::new(UnixDaemonClient::new(central_socket)),
            gateway_client: Arc::new(HttpDaemonClient::new(gateway_url)),
        }
    }

    pub fn spawn_agent(&mut self, _task: String) -> Uuid {
        let id = Uuid::new_v4();
        let agent = AgentEngine::new(
            id, 
            self.analyzer_client.clone(), 
            self.command_client.clone(),
            self.central_client.clone(),
            self.gateway_client.clone(),
        );
        self.agents.insert(id, agent);
        id
    }

    pub async fn handle_user_response(&mut self, agent_id: Uuid, text: String) -> Result<()> {
        if let Some(agent) = self.agents.get_mut(&agent_id) {
            agent.trajectory.add_message(Role::User, json!(text), text.len() / 4);
            agent.run_turn().await?;
        }
        Ok(())
    }

    pub fn emit_event(&self, event: CoreEvent) -> Result<()> {
        println!("{}", serde_json::to_string(&event)?);
        Ok(())
    }
}
