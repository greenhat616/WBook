# Session 管线：Session 运行时层

- 日期：2026-10-04
- 状态：草案
- 总览：[design.md](design.md)；领域层：[design-workspace.md](design-workspace.md)

## 1. 定位

Session 在一个进程中承载一个 Workspace。它不理解任何领域规则，只负责：单操作准入、在阻塞线程上以独占所有权执行操作、取消投递、完成凭据、快照、panic 隔离和关闭。Manager 负责注册与准入边界，Wbook 拥有 Manager。

## 2. 结构

```rust
#[derive(Clone)]
pub struct SessionHandle(Arc<Shared>);

struct Shared {
    id: SessionId,
    workspace_id: WorkspaceId,
    source: Utf8PathBuf,
    runtime: tokio::runtime::Handle,
    // Short bookkeeping only; never held across an await or a workspace call.
    inner: Mutex<Inner>,
    snapshot: watch::Sender<SessionSnapshot>,
    registry: Weak<ManagerShared>,
}

struct Inner {
    lifecycle: Lifecycle,
    slot: Slot,
    next_op: u64,
    last: Option<OperationSummary>,
}

enum Lifecycle {
    Open,
    Closing,
    Closed(Arc<CloseReport>),
}

enum Slot {
    Idle(Box<Workspace>),
    Busy(ActiveOp),
    Lost,
}

struct ActiveOp {
    id: OperationId,
    kind: OpKind,
    phase: Option<Phase>,
    ct: CancellationToken,
}
```

两个维度正交，组合后的行为：

| 生命周期 | 槽位 | 数据操作    | 查询 / 订阅 / 取消 / 关闭 |
| -------- | ---- | ----------- | ------------------------- |
| Open     | Idle | 接受        | 可用                      |
| Open     | Busy | Busy        | 可用                      |
| Open     | Lost | Unavailable | 可用                      |
| Closing  | 任意 | Closing     | 可用；关闭等待同一结果    |
| Closed   | —    | Closed      | 查询与关闭结果可用        |

Session 生命周期只有 Open → Closing → Closed，不会回到 Open。导出等操作完成不改变生命周期。

## 3. 通用执行

```rust
fn run<T: Send + 'static>(
    &self,
    kind: OpKind,
    op: impl FnOnce(&mut Workspace, &OpContext) -> Result<T, WorkspaceError> + Send + 'static,
) -> Result<Receipt<T>, Rejected>
```

1. **准入**（持锁）：按上表检查生命周期和槽位；通过则取出 Workspace，分配 OperationId，创建新的 CancellationToken，槽位置为 Busy，发布快照。准入判断在锁内完成，不依赖调用方看到的快照。
2. **执行**：在 `runtime` 上 spawn 一个任务，任务内 `spawn_blocking` 执行 `op(&mut workspace, &cx)`，结束后交回 Workspace 和结果。业务失败同样交回 Workspace，已提交的修改不会丢失。
3. **完成收口**（持锁，只有这一处）：
   - 正常返回：槽位放回 Idle；生命周期已是 Closing 时也放回，由关闭任务取走。
   - JoinError（panic 或运行时关闭）：槽位置为 Lost，结果为 `OpError::Panicked`。
   - 读取 Workspace 的 Revision、status 与 `take_warnings()`，组成 OperationResult；更新 `last`；先发布快照，再完成凭据。

公开方法都是对 `run` 的一行转发，替代原先的 Command 枚举与 controller 分发：

```rust
impl SessionHandle {
    pub fn apply_edits(&self, expected: Revision, batch: EditBatch) -> Result<Receipt<Revision>, Rejected> {
        self.run(OpKind::Edit, move |ws, cx| ws.apply_edits(cx, expected, batch).map(|(rev, _)| rev))
    }
}
```

数据操作清单与 Workspace 方法一一对应：initialize、parse、install、apply_edits、set_metadata_overrides、read_text、read_results、render_preview、export_epub。读取同样经过 `run`，因为执行期间 Workspace 已移交给工作线程。

扩展点：

- **并发配额**（本期不做）：在第 2 步 `spawn_blocking` 之前获取 Semaphore 许可，与令牌一起 `select!` 以保证等待中可取消，并增加 `Phase::WaitingForCapacity`。不影响其他接口。
- **保存**（持久化 spec）：`save` 是一个新的只读转发方法，经 `run` 获得独占所有权，因此只会看到提交边界上的状态。

## 4. 进度与快照

`OpContext::report` 闭包持有 `Weak<Shared>` 与本操作 OperationId：短暂加锁，只有活动操作 ID 匹配时才更新阶段并 `send_modify` 快照。迟到的进度因 ID 不匹配被丢弃，不会覆盖后续操作或终态。

```rust
pub struct SessionSnapshot {
    pub seq: u64,
    pub session: SessionId,
    pub workspace: WorkspaceId,
    pub source: Utf8PathBuf,
    pub lifecycle: LifecycleState,     // Open | Closing | Closed
    pub activity: Activity,            // Idle | Running { op, kind, phase, cancel_requested }
    pub workspace_status: Availability, // Available(WorkspaceStatus) | Lost { last_revision }
    pub last: Option<OperationSummary>,
}
```

- 快照派生 serde / specta，是接口 DTO，不是保存格式。
- 不包含正文或整棵目录；这些只通过数据读取操作返回。
- 操作运行期间 `workspace_status` 保持准入时的值，阶段是唯一的实时进度；操作结束时用交回的 Workspace 重新计算。
- `subscribe()` 返回 `watch::Receiver`，订阅时即持有完整当前值；慢订阅者看到的序号可以跳跃，表示中间更新被合并。

## 5. 操作结果与凭据

```rust
pub struct Receipt<T> {
    pub op: OperationId,
    rx: oneshot::Receiver<OperationResult<T>>,
}

pub struct OperationResult<T> {
    pub op: OperationId,
    pub kind: OpKind,
    pub revision: Revision,
    pub outcome: Result<T, OpError>,
    pub warnings: Vec<CleanupFailure>,
}

pub enum OpError {
    Workspace(WorkspaceError),
    Panicked,
}
```

- 每个凭据只由完成收口发送一次；后续操作不会影响已发出的凭据。
- 丢弃凭据不取消操作。
- `revision` 是操作结束时的实际值，调用方据此得知失败或取消前已提交了多少。
- 快照中的 OperationSummary 只保留 op、kind、revision 与结果类别：Succeeded、Failed、Cancelled（由 `WorkspaceError::is_cancelled` 判定）、Panicked。

## 6. 取消

`cancel(op) -> CancelReply`：持锁比较活动 OperationId，匹配则取消令牌并标记 `cancel_requested`，返回 Requested；重复取消同一活动操作幂等；不匹配（已结束或旧 ID）返回 NotActive。取消不经过工作线程，也不等待操作结束；最终结果以完成收口为准。

## 7. panic 隔离

工作线程 unwind panic 时，Workspace 随 unwind 一起被丢弃（预览临时目录由 Drop 尽力清理），槽位置为 Lost，不尝试复用不确定的数据。Lost 的 Session 拒绝数据操作，允许查询与关闭；其他 Session 不受影响。

`OpError::Panicked` 一律表示副作用未知：若 panic 发生在 EPUB 发布之后，目标文件可能已经存在，调用方根据自己请求的目标路径核对，运行时不删除也不宣称未生成。abort、OOM 与进程终止不在隔离保证内。以后有持久化时，Lost 工作区的恢复方式是从最近一次保存重新打开。

## 8. 关闭

`close(&self) -> Arc<CloseReport>`（async）：

1. 持锁：Open 时改为 Closing，取消活动操作的令牌，spawn 关闭任务；已是 Closing 或 Closed 则什么都不做。发布快照。
2. 等待快照生命周期变为 Closed，返回其中的同一份 CloseReport。

关闭任务：

1. 等待快照中的活动操作变为 Idle（`watch::Receiver::wait_for`），即实际工作线程已经返回并完成收口。
2. 取出槽位：Idle 得到 Workspace，Lost 得到 None。
3. 在阻塞线程中调用 `Workspace::close`，收集清理失败。以后的"关闭前保存"插在本步之前。
4. 置为 `Closed(report)`，发布快照，通过 `Weak` 从 Manager 注册表移除自己。

```rust
pub struct CloseReport {
    pub last: Option<OperationSummary>,
    pub lost: bool,
    pub cleanup_failures: Vec<CleanupFailure>,
}
```

- 关闭任务是独立 spawn 的，调用方放弃等待不会中止关闭；并发 close 读取同一个 `Arc<CloseReport>`。
- 关闭期间正在发布的导出照常完成，其凭据报告成功；关闭不删除已发布产物或输入文件。
- 不设强杀超时；调用方可以停止等待并展示"仍在关闭"，但不能据此当作 Closed。
- 普通 Handle 或订阅者的 Drop 不关闭 Session。已移除的 Session，旧 Handle 仍可读取终态快照与关闭结果，数据操作返回 Closed。

## 9. Manager 与 Wbook

```rust
pub struct SessionManager(Arc<ManagerShared>);

struct ManagerShared {
    runtime: tokio::runtime::Handle,
    registry: Mutex<Registry>,
}

struct Registry {
    accepting: bool,
    next_id: u64,
    sessions: HashMap<SessionId, SessionHandle>,
}
```

| 接口                    | 行为                                                                                      |
| ----------------------- | ----------------------------------------------------------------------------------------- |
| open(Workspace)         | 同一把锁内检查 `accepting`、分配 SessionId 并注册；停止中返回 ShuttingDown                |
| create(source, options) | `Workspace::new` 后调用 open；配置错误返回 InvalidConfig 且不注册                         |
| get(id) / list()        | 返回 Handle / 全部快照；已移除返回 NotFound                                               |
| close(id)               | 查找后转发 `SessionHandle::close`                                                         |
| shutdown()              | 锁内置 `accepting = false` 并取得全部 Handle；先对全部 Session 发出关闭，再统一等待并汇总 |

创建准入与 shutdown 共用同一把锁，因此不会遗漏 shutdown 期间被接受的 Session。先全部发出关闭再等待，避免一个阻塞操作拖延其他 Session 收到取消。

Manager 与 Wbook 的 Drop 只做尽力停止：置 `accepting = false` 并对全部 Session 发出关闭请求，不阻塞析构；只有显式 await `shutdown` 才保证全部结束。

Wbook 持有 Params 与 SessionManager，构造时取得当前 tokio 运行时句柄。`Params.data_dir` 本期不使用，以后作为工作区存储位置。

持久化相关：重启时需要恢复的"打开集"是工作区标识与存储位置的列表，由以后的持久化 spec 在 Manager / 应用层保存；SessionId 不保存，重新打开得到新的 SessionId。同一 WorkspaceId 重复打开的拒绝（AlreadyOpen）随恢复一起实现。

## 10. 错误

| 类型         | 何时返回                       | 变体                                  |
| ------------ | ------------------------------ | ------------------------------------- |
| Rejected     | 数据操作准入，同步返回         | Busy、Unavailable、Closing、Closed    |
| OpError      | 已接受操作的执行结果，在凭据中 | Workspace(WorkspaceError)、Panicked   |
| ManagerError | Manager 入口                   | NotFound、ShuttingDown、InvalidConfig |

入口拒绝与执行失败分开。领域错误全部来自 WorkspaceError，运行时不新增领域判断。

## 11. 测试

`run` 对操作闭包是泛型的，竞争测试直接提交受 barrier / channel 控制的合成闭包，无需在 Workspace 中加入故障注入钩子：Busy 与准入、按 OperationId 取消及旧 ID、panic 后 Lost、运行中关闭、并发关闭、创建与 shutdown 竞争、丢弃凭据、快照先于凭据、迟到进度不覆盖。超时只作为测试卡死的上限，不以睡眠判断执行阶段。
