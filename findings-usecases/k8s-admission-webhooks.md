# Kubernetes Admission Webhooks — Condensed

**5 design lessons for your DSL from production-scale infrastructure.**

| Feature | K8s | Your DSL |
|---------|-----|----------|
| Mutate (transform) + Validate (block) | Two phases, all mutate first | Actions (observe) + Filters (transform/block) |
| `failurePolicy: Fail` | Deny on webhook error | **`fail_closed` for security hooks** |
| `failurePolicy: Ignore` | Allow on webhook error | **`fail_open` for observability hooks** |
| `reinvocationPolicy: IfNeeded` | Re-run if prior hooks changed object | Re-evaluate when context changes |
| `matchConditions` | CEL expressions filter requests | Declarative `tools: ["bash"]` filtering |
| Phase ordering | Arbitrary within phase | **Phase > per-hook priority** |

**Lesson**: Every hook needs a failure mode declaration. K8s learned this after production outages where a misconfigured webhook silently allowed dangerous operations.
