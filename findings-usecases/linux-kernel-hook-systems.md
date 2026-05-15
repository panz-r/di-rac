# Linux Kernel Hook Systems — Netfilter & Seccomp

**Two production-proven hook systems at the lowest level of the software stack. Running on every Linux machine for 20+ years.**

## Netfilter: 5 Hook Points, Numeric Priority

```c
// 5 fixed hook points embedded in the IP stack
enum nf_inet_hooks {
    NF_INET_PRE_ROUTING,  // Before routing decision
    NF_INET_LOCAL_IN,     // After routing, destined for local
    NF_INET_FORWARD,      // After routing, destined for elsewhere
    NF_INET_LOCAL_OUT,    // From local process, before routing
    NF_INET_POST_ROUTING, // After routing, before wire
};

// Register callback with priority
struct nf_hook_ops {
    struct list_head list;         // Priority-ordered linked list
    nf_hookfn *hook;               // Callback function
    struct net_device *dev;        // Device filter (optional)
    struct net *net;               // Network namespace
    int pf;                        // Protocol family
    int priority;                  // Lower = earlier execution
};

// Return values
#define NF_DROP     0  // Block — drop the packet
#define NF_ACCEPT   1  // Allow — continue processing
#define NF_STOLEN   2  // Take ownership (don't continue)
#define NF_QUEUE    3  // Queue to userspace for decision
#define NF_REPEAT   4  // Re-run this hook
```

### Key Design Decisions

| Decision | Netfilter | For Your DSL |
|----------|-----------|-------------|
| **Hook points** | 5 fixed, embedded in IP stack | Fixed points in agent loop |
| **Priority** | Numeric (lower = earlier) | Numeric priority (default 10) |
| **Ordering** | Priority-ordered list | Same — predictably ordered |
| **Return values** | DROP, ACCEPT, STOLEN, QUEUE, REPEAT | Block, Allow, BlockAndQueue, Transform, Rerun |
| **Filtering** | Per-protocol + per-device | Per-tool-name + per-provider |
| **Scoping** | Network namespace | Session scope |

### Return Values Map Directly

| Netfilter | Agent Hook Equivalent |
|-----------|----------------------|
| **NF_DROP** | Block tool call with error |
| **NF_ACCEPT** | Allow execution to continue |
| **NF_STOLEN** | Take over and provide result directly |
| **NF_QUEUE** | Async user approval (like Hermes `pre_approval_request`) |
| **NF_REPEAT** | Re-run validation after other hooks modify context |

## Seccomp: BPF Filter + Actions

```c
// Seccomp filter actions
#define SECCOMP_RET_KILL_PROCESS  0x80000000  // Terminate process
#define SECCOMP_RET_KILL_THREAD   0x00000000  // Terminate thread
#define SECCOMP_RET_TRAP          0x00030000  // Send SIGSYS
#define SECCOMP_RET_ERRNO         0x00050000  // Return errno
#define SECCOMP_RET_TRACE         0x7ff00000  // Notify tracer
#define SECCOMP_RET_ALLOW         0x7fff0000  // Allow syscall
```

### Key Pattern: Action Escalation

Seccomp actions are ordered from strictest to most permissive. The filter returns the STRICTEST matching action. This is the **deny-wins** pattern — if any filter says KILL, that wins over ALLOW.

**Your DSL equivalent**: If any `before_tool_call` hook returns Block, that's the final answer. Deny-wins is the simplest and safest composition model.

## 5 Design Lessons for Your DSL

1. **Fixed hook points are better than dynamic** — Netfilter has exactly 5 points in the IP stack. You don't register new hook points; you register callbacks at existing ones. Same principle: define your loop's fixed stages; hooks attach to those stages.
2. **Numeric priority with predictable ordering** — Netfilter's priority-ordered list is simple and battle-tested. Lower = earlier. No hidden magic.
3. **5 return values cover all cases** — DROP, ACCEPT, STOLEN, QUEUE, REPEAT. Your hooks likely only need: Block, Allow, Queue (approval), TakeOver (synthetic result).
4. **Deny-wins is the safest default** — Strictest action wins. Seccomp proved this at the kernel level for 15+ years.
5. **Scoping prevents conflicts** — Netfilter's per-net namespace scoping prevents hooks in one container from affecting another. Your per-session scope is the same idea.
