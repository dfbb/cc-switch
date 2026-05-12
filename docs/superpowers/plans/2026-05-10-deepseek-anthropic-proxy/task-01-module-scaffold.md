# T01: 模块骨架

> **并行：** 串行，必须首先执行。所有其他任务依赖此任务完成。

**Goal:** 创建 `deepseek_anthropic` 子模块目录及全部空文件，并在 `providers/mod.rs` 中声明模块。此任务完成后，后续所有任务可并行开始。

**Files:**
- Create: `src-tauri/src/proxy/providers/deepseek_anthropic/mod.rs`
- Create: `src-tauri/src/proxy/providers/deepseek_anthropic/model_mapping.rs`
- Create: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`
- Create: `src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs`
- Create: `src-tauri/src/proxy/providers/deepseek_anthropic/sse_state.rs`
- Create: `src-tauri/src/proxy/providers/deepseek_anthropic/sse_stream.rs`
- Create: `src-tauri/src/proxy/providers/deepseek_anthropic/response_patch.rs`
- Modify: `src-tauri/src/proxy/providers/mod.rs`

---

- [ ] **Step 1: 创建模块目录**

```bash
mkdir -p src-tauri/src/proxy/providers/deepseek_anthropic
```

- [ ] **Step 2: 创建 `mod.rs`（公开 API + 子模块重导出）**

`src-tauri/src/proxy/providers/deepseek_anthropic/mod.rs`:

```rust
pub mod model_mapping;
pub mod request_sanitizer;
pub mod tool_repair;
pub mod sse_state;
pub mod sse_stream;
pub mod response_patch;

pub use model_mapping::map_claude_to_deepseek;
pub use request_sanitizer::{sanitize_request, SanitizeResult};
pub use sse_stream::{patch_sse_event, wrap_sse_stream};
pub use response_patch::patch_non_streaming_response;
```

- [ ] **Step 3: 创建各子模块空文件**

`src-tauri/src/proxy/providers/deepseek_anthropic/model_mapping.rs`:
```rust
// TODO: 由 T02 实现
```

`src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`:
```rust
// TODO: 由 T04a-T07 实现
```

`src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs`:
```rust
// TODO: 由 T08-T11 实现
```

`src-tauri/src/proxy/providers/deepseek_anthropic/sse_state.rs`:
```rust
// TODO: 由 T12 实现
```

`src-tauri/src/proxy/providers/deepseek_anthropic/sse_stream.rs`:
```rust
// TODO: 由 T13 实现
```

`src-tauri/src/proxy/providers/deepseek_anthropic/response_patch.rs`:
```rust
// TODO: 由 T03 实现
```

- [ ] **Step 4: 在 `providers/mod.rs` 声明模块**

在 `src-tauri/src/proxy/providers/mod.rs` 现有 `mod` 声明块末尾追加：

```rust
pub mod deepseek_anthropic;
```

- [ ] **Step 5: 验证可以编译（仅检查模块声明，忽略 TODO 内容）**

```bash
cd src-tauri && cargo check 2>&1 | head -30
```

Expected: 可能有 unused import warn，但 **无 error**。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/ src-tauri/src/proxy/providers/mod.rs
git commit -m "feat: scaffold deepseek_anthropic provider module"
```
