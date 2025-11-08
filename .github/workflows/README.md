# CI/CD Workflows

本项目使用 GitHub Actions 实现自动化构建和发布。

## 工作流说明

### 1. CI 工作流 (`ci.yml`)

**触发条件：**
- 推送到 `main`、`master` 或 `develop` 分支
- 针对这些分支的 Pull Request

**功能：**
- **测试任务**：在 Linux、macOS 和 Windows 上运行测试
- **代码检查**：运行 `cargo fmt` 和 `cargo clippy` 检查代码质量

**支持平台：**
- Ubuntu (Linux)
- macOS
- Windows

### 2. Release 工作流 (`release.yml`)

**触发条件：**
- 推送带有 `v*.*.*` 格式的 tag（如 `v0.1.0`）
- 手动触发（workflow_dispatch）

**功能：**
- 自动创建 GitHub Release
- 为多个平台构建优化的可执行文件
- 自动上传构建产物到 Release

**支持平台和架构：**
- Linux x86_64 (`pingultra-linux-x86_64.tar.gz`)
- Linux ARM64 (`pingultra-linux-aarch64.tar.gz`)
- macOS x86_64 Intel (`pingultra-macos-x86_64.tar.gz`)
- macOS ARM64 Apple Silicon (`pingultra-macos-aarch64.tar.gz`)
- Windows x86_64 (`pingultra-windows-x86_64.exe.zip`)

## 如何发布新版本

### 方法 1：使用 Git Tag（推荐）

```bash
# 1. 更新 Cargo.toml 中的版本号
# 2. 提交更改
git add Cargo.toml
git commit -m "Bump version to 0.1.0"

# 3. 创建并推送 tag
git tag v0.1.0
git push origin v0.1.0

# 4. GitHub Actions 会自动构建并创建 Release
```

### 方法 2：手动触发

1. 进入 GitHub 仓库的 Actions 页面
2. 选择 "Release" 工作流
3. 点击 "Run workflow" 按钮
4. 选择分支并运行

## 构建产物说明

### Linux 和 macOS

下载 `.tar.gz` 文件后解压：

```bash
tar xzf pingultra-linux-x86_64.tar.gz
chmod +x pingultra
sudo ./pingultra --help
```

### Windows

下载 `.zip` 文件后解压，直接运行 `pingultra.exe`（需要管理员权限）。

## 本地测试构建

如果想在本地测试多平台构建，可以使用 `cross`：

```bash
# 安装 cross
cargo install cross

# 构建 Linux ARM64 版本
cross build --release --target aarch64-unknown-linux-gnu

# 构建 Windows 版本（在 Linux/macOS 上）
cross build --release --target x86_64-pc-windows-gnu
```

## 缓存优化

工作流使用了 Cargo 缓存来加速构建：
- `~/.cargo/registry` - 依赖包缓存
- `~/.cargo/git` - Git 依赖缓存
- `target` - 编译产物缓存

## 故障排查

### 构建失败

1. 检查 Actions 日志查看具体错误
2. 确保所有依赖在目标平台上可用
3. 对于 Linux，确保 `libpcap-dev` 已安装

### Release 创建失败

1. 确保 tag 格式正确（`v*.*.*`）
2. 检查是否有权限创建 Release
3. 确保 `GITHUB_TOKEN` 有足够权限

### 交叉编译问题

对于 ARM64 Linux 构建，工作流会自动安装交叉编译工具链。如果失败，可能需要：
- 检查 `gcc-aarch64-linux-gnu` 是否正确安装
- 验证链接器配置是否正确

## 自定义配置

### 添加新平台

在 `release.yml` 的 `matrix.include` 中添加新条目：

```yaml
- os: ubuntu-latest
  target: armv7-unknown-linux-gnueabihf
  artifact_name: pingultra
  asset_name: pingultra-linux-armv7
```

### 修改触发条件

编辑工作流文件的 `on` 部分来改变触发条件。

### 添加构建选项

在 `cargo build` 命令中添加特性标志：

```yaml
run: cargo build --release --target ${{ matrix.target }} --features "your-feature"
```
