# Rust Hook System — di-core

## Design

Tower-inspired Service/Layer pattern, but adapted: engine loop calls hook "phases"
not individual Services. Each phase is a `Vec<Box<dyn HookPhase>>` executed in
registration order, with deny-wins composition for Filters.

## Core Types

```rust
// ── Hook identity ──

pub struct HookId {
    pub name: &'static str,
    pub version: &'static str,
}

// ── Two hook types ──

/// Action: observe, void return, never blocks.
#[async_trait]
pub trait ActionHook<C: Send + Sync>: Send + Sync {
    fn id(&self) -> HookId;
    fn fail_mode(&self) -> FailMode { FailMode::Open }
    async fn call(&self, ctx: &C);
}

/// Filter: transform or block, returns modified context or deny.
#[async_trait]
pub trait FilterHook<I: Send + Sync, O: Send + Sync>: Send + Sync {
    fn id(&self) -> HookId;
    fn fail_mode(&self) -> FailMode { FailMode::Open }
    async fn call(&self, input: I) -> FilterResult<O>;
}

// ── Composition ──

pub enum FilterResult<T> {
    /// Pass through (unmodified or modified)
    Continue(T),
    /// Block with reason
    Deny { reason: String },
}

#[derive(Clone, Copy)]
pub enum FailMode {
    /// On error, allow the operation (default for observability)
    Open,
    /// On error, deny the operation (security-critical hooks)
    Closed,
}

// ── Hook contexts ──

pub struct BeforeTurnContext {
    pub agent_id: Uuid,
    pub turn_number: usize,
    pub trajectory: &Trajectory,
    pub mode: AgentMode,
}

pub struct AfterTurnContext {
    pub agent_id: Uuid,
    pub turn_number: usize,
    pub outcome: TurnOutcome,
    pub tools_used: usize,
}

pub struct ToolCallContext {
    pub agent_id: Uuid,
    pub call: ToolCall,
    pub turn_number: usize,
}

pub struct ToolResultContext {
    pub agent_id: Uuid,
    pub tool_name: String,
    pub result: serde_json::Value,
    pub duration_ms: u64,
    pub error: Option<String>,
}

pub struct GatewayRequestContext {
    pub agent_id: Uuid,
    pub request: GatewayRequest,
}

pub struct ContextFrameContext {
    pub agent_id: Uuid,
    pub system_parts: Vec<String>,
    pub dynamic: DynamicContext,
}

pub struct ErrorContext {
    pub agent_id: Uuid,
    pub error: String,
    pub turn_number: usize,
    pub consecutive_errors: u32,
}

pub struct ApprovalPolicyInput {
    pub agent_id: Uuid,
    pub tool_name: String,
    pub args: serde_json::Value,
}

// ── Hook Registry ──

pub struct HookRegistry {
    before_turn: Vec<Box<dyn ActionHook<BeforeTurnContext>>>,
    after_turn: Vec<Box<dyn ActionHook<AfterTurnContext>>>,
    filter_tool_call: Vec<Box<dyn FilterHook<ToolCallContext, ToolCallContext>>>,
    before_tool_exec: Vec<Box<dyn ActionHook<ToolCallContext>>>,
    after_tool_exec: Vec<Box<dyn ActionHook<ToolResultContext>>>,
    filter_tool_result: Vec<Box<dyn FilterHook<ToolResultContext, ToolResultContext>>>,
    filter_gateway_request: Vec<Box<dyn FilterHook<GatewayRequestContext, GatewayRequestContext>>>,
    filter_context_frame: Vec<Box<dyn FilterHook<ContextFrameContext, ContextFrameContext>>>,
    before_compaction: Vec<Box<dyn ActionHook<CompactionContext>>>,
    after_compaction: Vec<Box<dyn ActionHook<CompactionContext>>>,
    filter_approval_policy: Vec<Box<dyn FilterHook<ApprovalPolicyInput, ApprovalPolicyInput>>>,
    on_error: Vec<Box<dyn ActionHook<ErrorContext>>>,
    on_recovery: Vec<Box<dyn ActionHook<ErrorContext>>>,
    on_session_event: Vec<Box<dyn ActionHook<SessionEventContext>>>,
}

impl HookRegistry {
    pub fn new() -> Self { /* empty vecs */ }

    // Registration
    pub fn register_before_turn(&mut self, hook: impl ActionHook<BeforeTurnContext> + 'static);
    pub fn register_filter_tool_call(&mut self, hook: impl FilterHook<ToolCallContext, ToolCallContext> + 'static);
    // ... one per hook point

    // Execution
    pub async fn run_before_turn(&self, ctx: &BeforeTurnContext);
    pub async fn run_filter_tool_call(&self, ctx: &mut ToolCallContext) -> FilterResult<()>;
    // ...
}

// ── Phase execution helpers ──

impl HookRegistry {
    /// Run all action hooks for a phase. Errors are logged and swallowed
    /// (fail_mode determines whether to log as warning or error).
    pub async fn run_actions<C: Send + Sync>(
        hooks: &[Box<dyn ActionHook<C>>],
        ctx: &C,
    ) {
        for hook in hooks {
            match hook.call(ctx).await {
                Ok(()) => {},
                Err(e) => match hook.fail_mode() {
                    FailMode::Open => tracing::warn!("hook {} failed (ignored): {}", hook.id().name, e),
                    FailMode::Closed => tracing::error!("hook {} failed: {}", hook.id().name, e),
                }
            }
        }
    }

    /// Run all filter hooks in sequence. Each receives the output of the previous.
    /// If any filter returns Deny, short-circuit.
    pub async fn run_filters<I, O>(
        hooks: &[Box<dyn FilterHook<I, O>>],
        input: I,
    ) -> FilterResult<O>
    where
        I: Send + Sync + Clone,
        O: Send + Sync,
    {
        let mut current = input;
        for hook in hooks {
            match hook.call(current).await {
                Ok(FilterResult::Continue(next)) => current = next,
                Ok(FilterResult::Deny { reason }) => return FilterResult::Deny { reason },
                Err(e) => match hook.fail_mode() {
                    FailMode::Open => continue,
                    FailMode::Closed => return FilterResult::Deny {
                        reason: format!("hook {} error: {}", hook.id().name, e),
                    },
                }
            }
        }
        FilterResult::Continue(current)
    }
}
```

## Integration into Engine Loop

```rust
// In run_turn():
let hook_ctx = BeforeTurnContext { agent_id, turn_number, trajectory, mode };
self.hooks.run_before_turn(&hook_ctx).await;

// Filter tool calls (replaces run_preflight_firewall):
for tool in &tools {
    let mut tc_ctx = ToolCallContext { call: tool.clone(), .. };
    match self.hooks.run_filter_tool_call(&mut tc_ctx).await {
        FilterResult::Continue(()) => {},
        FilterResult::Deny { reason } => { skip tool with reason },
    }
}

// Filter context frame:
let mut frame_ctx = ContextFrameContext { system_parts, dynamic };
match self.hooks.run_filter_context_frame(&mut frame_ctx).await {
    FilterResult::Continue(()) => build_system(frame_ctx.system_parts),
    FilterResult::Deny { reason } => error(reason),
}
```

## Compile-time Hook Composition (Alternative)

For hooks known at compile time, use Tower-style `Layer`:

```rust
pub trait ToolCallLayer: Send + Sync {
    fn layer(&self, inner: ToolCallService) -> Box<dyn ToolCallService>;
}

#[async_trait]
pub trait ToolCallService: Send + Sync {
    async fn call(&self, ctx: ToolCallContext) -> ServiceResult<FilterResult<ToolCallContext>>;
}
```

This is the opt-in path for performance-critical hooks (0.018ms overhead vs 37ms for subprocess).
