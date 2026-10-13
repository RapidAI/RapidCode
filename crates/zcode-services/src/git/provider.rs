//! Git 可执行文件的发现与命令执行。
//!
//! Rust 移植自 `packages/services/src/git/providers/gitEnvironmentProvider.ts`
//! 与 `packages/services/src/git/providers/gitCommandProvider.ts`。
//!
//! ## 为什么值得单独一个模块
//!
//! 「跑一条 git 命令」看着简单，但上游在这两个文件里处理了四类会静默出错的场景，
//! 这里一并保留：
//!
//! 1. **串仓**：pre-push hook 会注入 `GIT_DIR` / `GIT_WORK_TREE`，不清掉就会操作到
//!    hook 所在仓库（见 [`super::config::apply_git_command_env`]）。
//! 2. **挂住**：`GIT_PAGER` 没设成 `cat` 时，`git` 会起分页器并永远等输入。
//! 3. **输出爆炸**：`git log` / `git diff` 在异常仓库上可以吐出 GB 级输出，
//!    所以按字节预算截断并立刻杀进程，而不是读完再判断。
//! 4. **超时后进程不退出**：Windows 上 Git 的 helper 可能持有 stdout/stderr 句柄，
//!    进程本体死了、管道不关，读线程会永远卡住 —— 这就是 `orphaned` 这个字段的意义。
//!
//! ## 与上游的实现差异
//!
//! - 上游超时分两阶段（`child.kill()` = SIGTERM → 宽限 → `taskkill /T /F`）。
//!   Rust 的 [`std::process::Child::kill`] 在 Windows 上直接就是
//!   `TerminateProcess`，没有「先礼后兵」的余地，因此这里只有一次 kill，
//!   `force_kill_attempted` 的含义收敛成「超时时确实调用了 kill」。
//! - spawn 失败时额外带上 [`GitSpawnErrorKind`]，判定「cwd 不存在」不再依赖 stderr 文案。

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::config::{
    apply_git_command_env, get_git_binary_candidates, DEFAULT_GIT_COMMAND_TIMEOUT_MS,
    DEFAULT_GIT_DISCOVERY_TIMEOUT_MS, DEFAULT_GIT_OUTPUT_BYTES,
};

/// 等待子进程退出的轮询间隔。
///
/// `std::process::Child` 没有「带超时地 wait」的接口，只能 `try_wait` 轮询。
/// 5 ms 的粒度对「用户点了刷新」这个场景足够，也不会明显吃 CPU。
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// 超时后等待进程真正退出的宽限期。
///
/// 进程本身已经被 `TerminateProcess` 掉了，这段等的是「管道被孙进程释放」。
const DEFAULT_TIMEOUT_KILL_GRACE_MS: u64 = 2_000;

/// spawn 失败时使用的退出码，与上游 `exitCode = -2` 对齐。
pub const SPAWN_FAILURE_EXIT_CODE: i32 = -2;

/// 一次 Git 命令的执行结果。
///
/// 字段与上游 `GitCommandExecutionResult` 一一对应，只去掉了 Node 特有的
/// `signal`（Rust 侧拿不到，`ExitStatus::code()` 在 Windows 上恒为 `Some`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCommandExecutionResult {
    pub binary_path: String,
    pub cwd: String,
    pub args: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    /// 退出码；被杀或 spawn 失败时为 `None`。
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub timed_out: bool,
    pub timeout_ms: u64,
    /// 超时发生在命令启动后多久。
    pub timeout_elapsed_ms: Option<u128>,
    /// 超时到最终返回之间的清理耗时。
    pub timeout_close_delay_ms: Option<u128>,
    /// 超时时是否调用了 kill（Rust 侧一次 kill 即强制杀）。
    pub force_kill_attempted: bool,
    /// 杀掉之后仍未收齐 stdout/stderr（管道被孙进程持有）。
    pub orphaned: bool,
    /// 输出超过字节预算，已截断。
    pub output_truncated: bool,
    /// spawn 阶段的错误（cwd 被删、可执行文件不可用等）。
    pub spawn_error: Option<GitSpawnError>,
}

impl GitCommandExecutionResult {
    /// 命令是否成功（退出码为 0 且没有超时/截断/spawn 错误）。
    pub fn is_success(&self) -> bool {
        !self.timed_out
            && !self.output_truncated
            && self.spawn_error.is_none()
            && self.exit_code == Some(0)
    }

    /// 能直接展示给用户的失败原因。
    ///
    /// 对应上游 `toResultMessage()`：优先 stderr，其次 stdout，最后退回退出码。
    pub fn message(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return stderr.to_string();
        }
        let stdout = self.stdout.trim();
        if !stdout.is_empty() {
            return stdout.to_string();
        }
        match self.exit_code {
            Some(code) => format!("exitCode={code}"),
            None => "exitCode=null".to_string(),
        }
    }
}

/// spawn 失败的归类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitSpawnErrorKind {
    /// 可执行文件本身找不到。
    NotFound,
    /// cwd 不存在 —— 测试清理临时目录、窗口切工作区时会撞上。
    MissingWorkingDirectory,
    Other,
}

/// spawn 阶段的具体错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitSpawnError {
    pub kind: GitSpawnErrorKind,
    pub message: String,
}

/// 执行 Git 命令时可能出现的「无法开始执行」类错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitCommandError {
    /// 候选列表里没有任何一个候选能跑通 `git --version`。
    GitBinaryUnavailable,
}

impl fmt::Display for GitCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitCommandError::GitBinaryUnavailable => write!(f, "Git binary is not available"),
        }
    }
}

impl std::error::Error for GitCommandError {}

/// 一条待执行的 Git 命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCommandRequest {
    pub cwd: PathBuf,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    /// 单条命令的环境变量覆盖。
    ///
    /// **在 [`apply_git_command_env`] 之后叠加**，所以能盖掉被清掉的
    /// `GIT_*`（例如部分提交要临时指定 `GIT_INDEX_FILE`）。
    /// 上游是 `{ ...createCommandEnv(), ...command.env }`，顺序一致。
    pub env: Vec<(OsString, OsString)>,
}

impl GitCommandRequest {
    /// 用默认超时与默认输出预算构造一条命令。
    pub fn new<I, S>(cwd: impl Into<PathBuf>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            cwd: cwd.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout_ms: DEFAULT_GIT_COMMAND_TIMEOUT_MS,
            max_output_bytes: DEFAULT_GIT_OUTPUT_BYTES,
            env: Vec::new(),
        }
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn with_max_output_bytes(mut self, max_output_bytes: usize) -> Self {
        self.max_output_bytes = max_output_bytes;
        self
    }

    /// 追加一条环境变量覆盖（同名后者胜）。
    pub fn with_env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }
}

/// Git 可执行文件发现 + 命令执行。
///
/// 内部持有二进制路径缓存：探测一次之后不再重复打 `git --version`。
/// 这个类型是 `Send + Sync`，可以放进 `Arc` 跨线程共享（应用层就是这么用的）。
#[derive(Debug)]
pub struct GitCommandProvider {
    /// 外层 `None` = 还没探测过；`Some(None)` = 探测过，结论是「没有可用的 git」。
    binary_cache: Mutex<Option<Option<String>>>,
    discovery_timeout_ms: u64,
    timeout_kill_grace_ms: u64,
}

impl Default for GitCommandProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GitCommandProvider {
    pub fn new() -> Self {
        Self {
            binary_cache: Mutex::new(None),
            discovery_timeout_ms: DEFAULT_GIT_DISCOVERY_TIMEOUT_MS,
            timeout_kill_grace_ms: DEFAULT_TIMEOUT_KILL_GRACE_MS,
        }
    }

    pub fn with_discovery_timeout_ms(mut self, discovery_timeout_ms: u64) -> Self {
        self.discovery_timeout_ms = discovery_timeout_ms;
        self
    }

    pub fn with_timeout_kill_grace_ms(mut self, timeout_kill_grace_ms: u64) -> Self {
        self.timeout_kill_grace_ms = timeout_kill_grace_ms;
        self
    }

    /// 清掉二进制路径缓存，下次调用重新探测。
    pub fn invalidate_binary_cache(&self) {
        let mut cache = lock_unpoisoned(&self.binary_cache);
        *cache = None;
    }

    /// 解析出可用的 Git 可执行文件，结果会被缓存。
    ///
    /// 对应上游 `resolveGitBinary()`：按候选顺序试 `--version`，第一个退出码为 0 的胜出。
    pub fn resolve_git_binary(&self) -> Option<String> {
        let mut cache = lock_unpoisoned(&self.binary_cache);
        if let Some(cached) = cache.as_ref() {
            return cached.clone();
        }

        let resolved = self.discover_git_binary();
        *cache = Some(resolved.clone());
        resolved
    }

    /// 执行一条 Git 命令。
    ///
    /// 只有「连 git 都找不到」才会返回 `Err`；其余失败（cwd 不存在、超时、输出超限）
    /// 都以结果对象的形式返回，交给 [`super::cli_helpers::ensure_git_command_succeeded`]
    /// 按统一语义处理 —— 这一点与上游一致，目的是不让异常在调用链里变成未捕获错误。
    pub fn run(
        &self,
        request: &GitCommandRequest,
    ) -> Result<GitCommandExecutionResult, GitCommandError> {
        let Some(binary_path) = self.resolve_git_binary() else {
            return Err(GitCommandError::GitBinaryUnavailable);
        };

        let started = Instant::now();
        let mut command = Command::new(&binary_path);
        command
            .args(&request.args)
            .current_dir(&request.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_git_command_env(&mut command);
        // 单条命令的覆盖要**后**加，否则会被上面的 env_remove 清掉。
        for (name, value) in &request.env {
            command.env(name, value);
        }
        hide_console(&mut command);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Ok(self.spawn_failure_result(&binary_path, request, started, error));
            }
        };

        // 两个管道各起一个读线程。读线程必须独立于等待循环，
        // 否则「子进程写满管道 → 等我们读」和「我们等子进程退出」会互锁。
        let truncated = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel::<PipeOutput>();
        let mut expected_pipes = 0usize;
        if let Some(pipe) = child.stdout.take() {
            expected_pipes += 1;
            spawn_pipe_reader(
                pipe,
                request.max_output_bytes,
                Arc::clone(&truncated),
                sender.clone(),
                PipeKind::Stdout,
            );
        }
        if let Some(pipe) = child.stderr.take() {
            expected_pipes += 1;
            spawn_pipe_reader(
                pipe,
                request.max_output_bytes,
                Arc::clone(&truncated),
                sender.clone(),
                PipeKind::Stderr,
            );
        }
        drop(sender);

        let deadline = started + Duration::from_millis(request.timeout_ms);
        let mut timed_out = false;
        let mut timeout_elapsed_ms: Option<u128> = None;
        let mut force_kill_attempted = false;
        let mut orphaned = false;
        let mut exit_code: Option<i32> = None;
        let mut killed_for_truncation = false;

        loop {
            if truncated.load(Ordering::SeqCst) && !killed_for_truncation {
                // 输出超限：与上游一样立刻杀掉，不继续读。
                let _ = child.kill();
                killed_for_truncation = true;
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_code = status.code();
                    break;
                }
                Ok(None) => {}
                Err(_) => break,
            }

            if Instant::now() >= deadline {
                timed_out = true;
                timeout_elapsed_ms = Some(started.elapsed().as_millis());
                let _ = child.kill();
                force_kill_attempted = true;
                exit_code = wait_with_grace(&mut child, self.timeout_kill_grace_ms);
                orphaned = exit_code.is_none();
                break;
            }

            std::thread::sleep(POLL_INTERVAL);
        }

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut output_truncated = false;
        let collect_deadline = Instant::now() + Duration::from_millis(self.timeout_kill_grace_ms);
        let mut received = 0usize;
        while received < expected_pipes {
            let remaining = collect_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match receiver.recv_timeout(remaining) {
                Ok(output) => {
                    received += 1;
                    output_truncated |= output.truncated;
                    match output.kind {
                        PipeKind::Stdout => stdout = output.text,
                        PipeKind::Stderr => stderr = output.text,
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        if received < expected_pipes {
            // 管道没关干净 —— 上游在这种情况下一律报 orphaned 且退出码为 null，
            // 让上层按失败处理，而不是拿着半个输出继续往下走。
            orphaned = true;
        }
        if orphaned {
            exit_code = None;
        }

        let duration_ms = started.elapsed().as_millis();
        Ok(GitCommandExecutionResult {
            binary_path,
            cwd: request.cwd.to_string_lossy().into_owned(),
            args: request.args.clone(),
            stdout,
            stderr,
            exit_code,
            duration_ms,
            timed_out,
            timeout_ms: request.timeout_ms,
            timeout_elapsed_ms,
            timeout_close_delay_ms: timeout_elapsed_ms
                .map(|elapsed| duration_ms.saturating_sub(elapsed)),
            force_kill_attempted,
            orphaned,
            output_truncated,
            spawn_error: None,
        })
    }

    fn discover_git_binary(&self) -> Option<String> {
        for candidate in get_git_binary_candidates() {
            if self.can_execute_candidate(&candidate) {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
        None
    }

    /// 探测一个候选能否跑通 `git --version`。
    fn can_execute_candidate(&self, candidate: &OsStr) -> bool {
        let mut command = Command::new(candidate);
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_git_command_env(&mut command);
        hide_console(&mut command);

        let Ok(mut child) = command.spawn() else {
            return false;
        };

        let deadline = Instant::now() + Duration::from_millis(self.discovery_timeout_ms);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return status.success(),
                Ok(None) => {}
                Err(_) => return false,
            }

            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }

            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn spawn_failure_result(
        &self,
        binary_path: &str,
        request: &GitCommandRequest,
        started: Instant,
        error: std::io::Error,
    ) -> GitCommandExecutionResult {
        let message = error.to_string();
        GitCommandExecutionResult {
            binary_path: binary_path.to_string(),
            cwd: request.cwd.to_string_lossy().into_owned(),
            args: request.args.clone(),
            stdout: String::new(),
            stderr: message.clone(),
            // 与上游一致：spawn 失败降级成「命令失败结果」而不是抛错。
            exit_code: Some(SPAWN_FAILURE_EXIT_CODE),
            duration_ms: started.elapsed().as_millis(),
            timed_out: false,
            timeout_ms: request.timeout_ms,
            timeout_elapsed_ms: None,
            timeout_close_delay_ms: None,
            force_kill_attempted: false,
            orphaned: false,
            output_truncated: false,
            spawn_error: Some(GitSpawnError {
                kind: classify_spawn_error(&error, &request.cwd),
                message,
            }),
        }
    }
}

/// 归类 spawn 错误。
///
/// 上游靠 stderr 里有没有 `ENOENT` 来区分「cwd 不存在」和「git 不存在」；
/// Rust 侧更可靠的判据是**直接看 cwd 在不在**：
/// Windows 上 `CreateProcessW` 对缺失目录返回的是 `ERROR_DIRECTORY`(267)，
/// Rust 把它归到 `Uncategorized`，光看 `io::ErrorKind` 是认不出来的。
fn classify_spawn_error(error: &std::io::Error, cwd: &Path) -> GitSpawnErrorKind {
    if !cwd.is_dir() {
        return GitSpawnErrorKind::MissingWorkingDirectory;
    }

    if error.kind() == std::io::ErrorKind::NotFound {
        return GitSpawnErrorKind::NotFound;
    }

    GitSpawnErrorKind::Other
}

/// 杀掉之后等进程真正退出；返回退出码，超时返回 `None`。
fn wait_with_grace(child: &mut std::process::Child, grace_ms: u64) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_millis(grace_ms);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) => {}
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 哪个管道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipeKind {
    Stdout,
    Stderr,
}

/// 一个管道读完之后的产物。
#[derive(Debug)]
struct PipeOutput {
    kind: PipeKind,
    text: String,
    truncated: bool,
}

/// 起一个线程把管道读干，最多读 `max_bytes`。
///
/// 超限时置 `truncated` 标志并停止读取（管道句柄随之关闭，子进程会收到写失败），
/// 由等待循环负责杀进程 —— 这与上游「发现超限就 `child.kill()`」等价。
fn spawn_pipe_reader<R>(
    mut pipe: R,
    max_bytes: usize,
    truncated: Arc<AtomicBool>,
    sender: mpsc::Sender<PipeOutput>,
    kind: PipeKind,
) where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; 16 * 1024];
        let mut collected: Vec<u8> = Vec::new();
        let mut was_truncated = false;

        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if collected.len() + read > max_bytes {
                        truncated.store(true, Ordering::SeqCst);
                        was_truncated = true;
                        break;
                    }
                    collected.extend_from_slice(&buffer[..read]);
                }
                Err(_) => break,
            }
        }

        let _ = sender.send(PipeOutput {
            kind,
            // Git 的输出是 UTF-8，但 Windows 上非 ASCII 路径可能带本地编码；
            // 上游用 Node 的 utf8 解码（同样是有损的），这里对齐成 lossy。
            text: String::from_utf8_lossy(&collected).into_owned(),
            truncated: was_truncated,
        });
    });
}

/// 加锁时忽略中毒状态：缓存里只有一个可选项，中毒也不会让它变成坏数据。
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 隐藏控制台窗口，对应 Node 的 `windowsHide: true`。
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本机有没有可用的 git。测试用外部命令时先问一句，
    /// 没有就打印一行说明并跳过 —— 比让测试环境相关地红掉好。
    fn git_available(provider: &GitCommandProvider) -> bool {
        let available = provider.resolve_git_binary().is_some();
        if !available {
            eprintln!("跳过：本机找不到可用的 git");
        }
        available
    }

    #[test]
    fn discovers_a_git_binary_and_caches_it() {
        let provider = GitCommandProvider::new();
        let Some(binary) = provider.resolve_git_binary() else {
            eprintln!("跳过：本机找不到可用的 git");
            return;
        };
        assert!(!binary.is_empty());
        // 第二次必须命中缓存，返回同一个值
        assert_eq!(provider.resolve_git_binary().as_deref(), Some(binary.as_str()));

        provider.invalidate_binary_cache();
        assert_eq!(provider.resolve_git_binary().as_deref(), Some(binary.as_str()));
    }

    #[test]
    fn runs_git_version_successfully() {
        let provider = GitCommandProvider::new();
        if !git_available(&provider) {
            return;
        }

        let result = provider
            .run(&GitCommandRequest::new(
                std::env::temp_dir(),
                ["--version"],
            ))
            .expect("git 可用时 run 不应返回 Err");

        assert!(result.is_success(), "stderr={}", result.stderr);
        assert!(result.stdout.starts_with("git version"), "{}", result.stdout);
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.timed_out);
        assert!(!result.output_truncated);
        assert!(result.spawn_error.is_none());
        assert_eq!(result.args, vec!["--version".to_string()]);
    }

    #[test]
    fn missing_working_directory_is_reported_as_spawn_error() {
        let provider = GitCommandProvider::new();
        if !git_available(&provider) {
            return;
        }

        // 一个必然不存在的目录。git 可用、cwd 不可用 —— 这正是「workspace 被删掉」的场景。
        let missing = std::env::temp_dir().join("zcode-services-definitely-missing-cwd");
        assert!(!missing.exists());

        let result = provider
            .run(&GitCommandRequest::new(missing, ["--version"]))
            .expect("git 可用时 run 不应返回 Err");

        let spawn_error = result
            .spawn_error
            .as_ref()
            .expect("应当带上 spawn 错误");
        assert_eq!(spawn_error.kind, GitSpawnErrorKind::MissingWorkingDirectory);
        assert_eq!(result.exit_code, Some(SPAWN_FAILURE_EXIT_CODE));
        assert!(!result.is_success());
        // 降级成结果而不是 Err —— 上游这么做是为了不让异常变成 unhandled rejection
        assert!(!spawn_error.message.is_empty());
    }

    #[test]
    fn message_falls_back_to_exit_code() {
        let result = GitCommandExecutionResult {
            binary_path: "git".into(),
            cwd: ".".into(),
            args: vec![],
            stdout: "  \n".into(),
            stderr: String::new(),
            exit_code: Some(7),
            duration_ms: 1,
            timed_out: false,
            timeout_ms: 1000,
            timeout_elapsed_ms: None,
            timeout_close_delay_ms: None,
            force_kill_attempted: false,
            orphaned: false,
            output_truncated: false,
            spawn_error: None,
        };
        assert_eq!(result.message(), "exitCode=7");
    }

    #[test]
    fn message_prefers_stderr() {
        let mut result = GitCommandExecutionResult {
            binary_path: "git".into(),
            cwd: ".".into(),
            args: vec![],
            stdout: "out".into(),
            stderr: "err".into(),
            exit_code: Some(1),
            duration_ms: 1,
            timed_out: false,
            timeout_ms: 1000,
            timeout_elapsed_ms: None,
            timeout_close_delay_ms: None,
            force_kill_attempted: false,
            orphaned: false,
            output_truncated: false,
            spawn_error: None,
        };
        assert_eq!(result.message(), "err");
        result.stderr.clear();
        assert_eq!(result.message(), "out");
    }

    #[test]
    fn request_builder_applies_defaults_and_overrides() {
        let request = GitCommandRequest::new("/tmp", ["status", "--porcelain=v2"]);
        assert_eq!(request.timeout_ms, DEFAULT_GIT_COMMAND_TIMEOUT_MS);
        assert_eq!(request.max_output_bytes, DEFAULT_GIT_OUTPUT_BYTES);
        assert_eq!(request.args.len(), 2);

        let request = request.with_timeout_ms(123).with_max_output_bytes(456);
        assert_eq!(request.timeout_ms, 123);
        assert_eq!(request.max_output_bytes, 456);
    }

    #[test]
    fn request_builder_accumulates_env_overrides() {
        let request = GitCommandRequest::new("/tmp", ["status"])
            .with_env("GIT_INDEX_FILE", "/tmp/index")
            .with_env("ZCODE_MARKER", "1");

        assert_eq!(request.env.len(), 2);
        assert_eq!(request.env[0].0, OsString::from("GIT_INDEX_FILE"));
        assert_eq!(request.env[0].1, OsString::from("/tmp/index"));
        assert_eq!(request.env[1].0, OsString::from("ZCODE_MARKER"));
    }

    /// 单条命令的 env 必须在 `apply_git_command_env` **之后**叠加。
    ///
    /// 这条测试直接看 `Command::get_envs()` 的最终结果：`GIT_INDEX_FILE` 在
    /// 清理清单里被 `env_remove`，只有后加的覆盖能把它救回来。
    #[test]
    fn per_command_env_overrides_the_sanitized_environment() {
        let mut command = Command::new("git");
        apply_git_command_env(&mut command);
        for (name, value) in [("GIT_INDEX_FILE", "/tmp/index")] {
            command.env(name, value);
        }

        let envs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();

        let entry = envs
            .iter()
            .find(|(key, _)| key == "GIT_INDEX_FILE")
            .expect("GIT_INDEX_FILE 应当出现在最终环境里");
        assert_eq!(entry.1.as_deref(), Some("/tmp/index"));

        // 清理清单里其它变量仍然是「移除」状态
        let dir_entry = envs
            .iter()
            .find(|(key, _)| key == "GIT_DIR")
            .expect("GIT_DIR 应当被显式处理");
        assert!(dir_entry.1.is_none(), "GIT_DIR 应当保持被移除");
    }

    #[test]
    fn command_error_display_matches_upstream_message() {
        assert_eq!(
            GitCommandError::GitBinaryUnavailable.to_string(),
            "Git binary is not available"
        );
    }

    #[test]
    fn classify_spawn_error_uses_cwd_existence() {
        let existing = std::env::temp_dir();
        let missing = existing.join("zcode-services-definitely-missing-cwd");

        // cwd 不存在 → 无论 ErrorKind 是什么都归成 MissingWorkingDirectory
        for kind in [
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::Other,
            std::io::ErrorKind::PermissionDenied,
        ] {
            assert_eq!(
                classify_spawn_error(&std::io::Error::from(kind), &missing),
                GitSpawnErrorKind::MissingWorkingDirectory,
                "{kind:?} + 缺失 cwd 应当判为 MissingWorkingDirectory"
            );
        }

        // cwd 存在 → 按 ErrorKind 走
        assert_eq!(
            classify_spawn_error(
                &std::io::Error::from(std::io::ErrorKind::NotFound),
                &existing
            ),
            GitSpawnErrorKind::NotFound
        );
        assert_eq!(
            classify_spawn_error(
                &std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                &existing
            ),
            GitSpawnErrorKind::Other
        );
    }
}
