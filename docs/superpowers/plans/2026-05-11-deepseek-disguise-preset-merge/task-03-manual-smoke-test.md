# T03 — 手工烟雾测试

**可并行：** 否
**依赖：** T02

**Files:**
- Verify: 应用运行时行为（无文件修改）

## 目标

确认合并后的预设在应用中可用、字段默认值正确、转发链路正常。

## 步骤

- [ ] **Step 1：构建并启动应用**

  ```bash
  npm run tauri build
  ```

  把生成的 `.app` 复制到 `/Applications` 并启动。

- [ ] **Step 2：UI 验证预设**

  - 打开 cc-switch
  - 点击「添加 Provider」→ 选择 Claude 类型
  - 在预设下拉里确认：
    - ✅ 看到 `DeepSeek (Claude Disguise)`
    - ✅ **看不到** `DeepSeek (Claude Disguise · Flash)`
    - ✅ **看不到** `DeepSeek (Claude Disguise · Pro)`
  - 选中该预设，确认表单字段值：

    ```
    ANTHROPIC_BASE_URL              = https://api.deepseek.com/anthropic
    ANTHROPIC_API_KEY               = (空)
    ANTHROPIC_MODEL                 = claude-opus-4-7
    ANTHROPIC_DEFAULT_HAIKU_MODEL   = claude-sonnet-4-6
    ANTHROPIC_DEFAULT_SONNET_MODEL  = claude-sonnet-4-6
    ANTHROPIC_DEFAULT_OPUS_MODEL    = claude-opus-4-7
    ```

- [ ] **Step 3：填 API Key 并保存**

  填入有效 DeepSeek key，保存 Provider，激活。

- [ ] **Step 4：curl 验证 Pro 路由**

  ```bash
  PORT=$(...)  # cc-switch 显示的代理端口
  curl -s http://127.0.0.1:$PORT/v1/messages \
    -H "Content-Type: application/json" \
    -d '{"model":"claude-opus-4-7","messages":[{"role":"user","content":"hi"}],"max_tokens":10}' \
    | python3 -m json.tool | grep '"model"'
  ```

  期望：`"model": "claude-opus-4-7"`（伪装名）。

- [ ] **Step 5：curl 验证 Flash 路由**

  ```bash
  curl -s http://127.0.0.1:$PORT/v1/messages \
    -H "Content-Type: application/json" \
    -d '{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"hi"}],"max_tokens":10}' \
    | python3 -m json.tool | grep '"model"'
  ```

  期望：`"model": "claude-sonnet-4-6"`。

- [ ] **Step 6：旧 Provider 回归（若有）**

  如果机器上之前已经用 Flash 或 Pro 预设建过 Provider：
  - 确认它们仍出现在 Provider 列表
  - 确认可以激活、可以正常请求

  （旧 Provider 的 `name` 字段是用户保存时拷贝的快照，不依赖预设是否存在。）

## 验收

- 预设下拉里只有一条 disguise 项
- 表单字段默认值与 spec 完全一致
- 两个 curl 测试都返回伪装的 Claude 模型名
- 旧 Provider 仍可用

## 完成后

使用 superpowers:finishing-a-development-branch 收尾（合并 / PR / 保留 / 丢弃）。
