# Agent Team 工作流指南

## 适用场景

| 场景 | 推荐模式 | 原因 |
|------|----------|------|
| 简单翻译/邮件 | 单 Agent | Token 效率高 |
| 代码质量审查 | 3 人 Team | 安全/性能/测试并行 |
| 多模块功能开发 | 3-5 人 Team | 前端/后端/算法/测试各一个 |
| PR Review | 2-3 人 Team | 不同视角并行审查 |
| 文档编写 | 单 Agent | 上下文一致性重要 |

## Team 创建模板

### 代码质量修复团队

```
Create a team for VoltageEMS code quality improvement:

@rust-expert: Focus on core Rust issues — concurrency bugs (seqlock, ringbuffer),
unsafe code review, and performance-critical paths.

@code-quality: Handle code standards — unwrap/panic cleanup, dead code removal,
naming conventions. Independent track, don't block others.

@test-expert: Write and verify tests for all fixes. Wait for dependencies from
rust-expert before testing those modules.

Keep the team alive after completing tasks. Do not shut down teammates.
```

### PR 并行审查团队

```
Create a review team for PR #[number]:

@security-reviewer: Focus on auth, data validation, injection risks.
@architecture-reviewer: Module boundaries, API contracts, backward compatibility.
@test-reviewer: Test coverage, edge cases, performance regression.

Each reviewer creates a summary. Don't merge — just report findings.
```

### 多模块功能开发团队

```
Create a development team for [feature name]:

@frontend: UI components. Coordinate with @backend on API contracts.
@backend: API endpoints and business logic. Define contracts early.
@algorithm: Core algorithm with clear interface spec.

Direct communication between teammates is encouraged.
```

## 核心设计原则

1. **分层解耦**：方向层 → 协调层 → 执行层
2. **依赖驱动**：让依赖关系驱动流转，不按人头平分
3. **水平沟通**：执行层之间直接对话，不必经过 lead
4. **只给方向**：Lead 只协调不做判断，判断权在各 expert

## Token 参考

3 人团队 ≈ 30万 tokens / 轮。适合真正需要并行的复杂任务。
