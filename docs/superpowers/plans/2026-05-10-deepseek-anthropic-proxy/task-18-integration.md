# T18: 集成验证

> **并行：** ⚠️ 需等 T15/T16/T17 全部完成。

**Goal:** 全量编译 + 测试确认 + 端到端冒烟，确保各模块端到端串通，无回归。

**Files:**
- 不新建文件；可按需修复各模块的集成 bug。

---

- [ ] **Step 1: 全量后端编译 + 测试**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: `test result: ok. N passed; 0 failed`（所有 crate 测试通过）。

若失败：定位报错模块，按编译错误信息修复（常见问题：`ForwardResult` 构造点漏填 `deepseek_context`、`process_response` 调用点漏加 `None`）。

- [ ] **Step 2: 后端 clippy lint**

```bash
cd src-tauri && cargo clippy -- -D warnings 2>&1 | tail -20
```

Expected: 无 error 级别 warning。  
若出现 `dead_code` / `unused_import`：按提示修复。

- [ ] **Step 3: 前端类型检查 + 单测**

```bash
pnpm typecheck 2>&1 | tail -5
pnpm test:unit 2>&1 | tail -10
```

Expected: `Found 0 errors`；`All tests passed`。

- [ ] **Step 4: 端到端冒烟（需要 DeepSeek API Key）**

若有 DeepSeek API Key，按以下步骤验证：

**4a. 启动开发模式：**

```bash
pnpm dev
```

**4b. 在 CC Switch UI 新增 provider：**

- 从预设选择 "DeepSeek (Claude Disguise · Flash)"
- 填入 `ANTHROPIC_API_KEY`（DeepSeek API Key）
- 保存并设为活动 provider

**4c. 验证 `/v1/models` 返回伪装列表：**

```bash
curl http://127.0.0.1:<proxy_port>/v1/models
```

Expected: `{"data":[{"type":"model","id":"claude-sonnet-4-6","display_name":"claude-sonnet-4-6"},...]}`

**4d. 验证非流式请求改写 model 名：**

```bash
curl -s http://127.0.0.1:<proxy_port>/v1/messages \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"hi"}],"max_tokens":10}' \
  | python3 -m json.tool | grep model
```

Expected: `"model": "claude-sonnet-4-6"`（伪装名，非 deepseek-v4-flash）

**4e. 验证流式请求 model 改写（SSE 第一个 message_start 事件）：**

```bash
curl -N -s http://127.0.0.1:<proxy_port>/v1/messages \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"hi"}],"max_tokens":10,"stream":true}' \
  2>&1 | head -5
```

Expected: 第一行包含 `"model": "claude-sonnet-4-6"`（非 `deepseek-v4-flash`）。

**4f. 若无 API Key：** 跳过步骤 4d/4e，仅运行 `cargo test` 确保全通过即可。

- [ ] **Step 5: 全量格式化 + 最终提交**

```bash
cd src-tauri && cargo fmt
cd .. && pnpm format
```

确认无未提交改动：

```bash
git status
```

若有格式化改动，提交：

```bash
git add -A
git commit -m "chore: format after deepseek_anthropic integration"
```

- [ ] **Step 6: 里程碑确认**

确认所有已实现内容覆盖 spec 要求：

- [x] `model_mapping.rs`：claude-* → deepseek-v4-*/flash 映射
- [x] `response_patch.rs`：非流式响应 model + thinking 过滤
- [x] `request_sanitizer.rs`：10 步净化流水线（含 tool_repair）
- [x] `tool_repair.rs`：build_plan + add_delete_ops + aggregate_paired_remaining + apply_plan
- [x] `sse_state.rs`：块策略状态机 + bypass mode
- [x] `sse_stream.rs`：wrap_sse_stream + patch_sse_event
- [x] `claude.rs`：`deepseek_anthropic` api_format 分支
- [x] `forwarder.rs`：sanitize_request 注入 + DeepseekContext 传递 + header 黑名单
- [x] `response_processor.rs`：wrap_sse_stream + patch_non_streaming_response 接入
- [x] `handlers.rs`：handle_claude_models + select_models_endpoint_provider + build_deepseek_disguised_models_payload
- [x] `server.rs`：/v1/models + /claude/v1/models 路由
- [x] 前端：types.ts + claudeProviderPresets.ts + ClaudeFormFields.tsx + i18n
