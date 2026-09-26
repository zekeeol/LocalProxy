# WireGuard helper 生命周期问题记录（锁屏断连 / 反复要管理员密码 / utun 残留）

记录时间：2026-09-24
相关代码：`rust/src/helper_paths.rs`、`rust/src/core/wireguard.rs`、`rust/src/bin/local_proxy_wireguard_helper.rs`、`rust/src/core/mod.rs`、`rust/src/main.rs`、`electron/main.ts`

> 状态：§1–§3 是问题与根因记录；§5 是同日落地的修复（R1/R2/R3/R5 及“记录”诉求），§7 是仍然开放的事项。

## 1. 现象

1. 锁屏（或合盖/休眠）之后 WireGuard 隧道不可用，界面仍可能显示“运行中”。
2. 每次（重新）启动隧道都要重新输入 **“script 要求管理员权限”** 的密码。
3. 系统里堆积了多个 `utun`，而且都持有同一个地址：

```text
utun5: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1420
        inet 192.168.1.6 --> 192.168.1.6 netmask 0xffffff00
utun6: ... mtu 1420  inet 192.168.1.6 --> 192.168.1.6
utun7: ... mtu 1420  inet 192.168.1.6 --> 192.168.1.6
utun9: ... mtu 1420  inet 192.168.1.6 --> 192.168.1.6
```

（`utun4/utun10`（mtu 1380）与 `utun8`（198.18.0.1）属于其它工具，不在本次问题范围。）

## 2. 现场证据（2026-09-24 15:0x）

### 2.1 有 7 个 root helper 同时存活，全部 PPID=1，共用同一个 socket 路径和 token

```text
$ ps -o pid,ppid,stat,lstart,command -p 30228,3614,3583,95252,89142,86502,86436
30228  1 S  四  9月/24 10:45:49 2026  .../release/mac-arm64/LocalProxy.app/.../bin/local_proxy_wireguard_helper --socket $TMPDIR/localproxy-wireguard/localproxy-wireguard.sock --token 1d18...
3614   1 S  二  9月/15 15:44:26 2026  .../rust/target/release/local_proxy_wireguard_helper ...
3583   1 S  二  9月/15 15:44:21 2026  .../release/mac-arm64/LocalProxy.app/.../bin/local_proxy_wireguard_helper ...
95252  1 S  二  9月/15 15:26:58 2026
89142  1 S  二  9月/15 15:13:44 2026
86502  1 S  二  9月/15 15:08:11 2026
86436  1 S  二  9月/15 15:07:59 2026
```

即：**应用每换一次 helper，旧 helper 都变成活的孤儿进程**，而且每个孤儿都还握着自己的 utun 设备。

### 2.2 每个存活设备都在 `/var/run/wireguard/` 留下一个 UAPI socket，时间戳与 utun 一一对应

```text
$ ls -la /var/run/wireguard/
srwxr-xr-x  1 root staff  utun5.sock  9月 15 15:30
srwxr-xr-x  1 root staff  utun6.sock  9月 15 15:43
srwxr-xr-x  1 root staff  utun7.sock  9月 15 15:44
srwxr-xr-x  1 root staff  utun9.sock  9月 24 11:05
```

4 个设备 ↔ 4 个 `192.168.1.6` 地址，其中 3 个是 9/15 的残留，只有 `utun9` 是当天的。

### 2.3 路由被“后来者”覆盖，只有最新那个设备在承载流量

```text
$ netstat -rn -f inet | grep -E '172.16.110|172.31.0.2'
172.16.110/24   utun9  USc
172.31.0.2      utun9  UHS
```

### 2.4 版本/构建时间（用于判断“运行中的 App 是否包含修复”）

```text
release/LocalProxy-0.2.0-arm64.dmg        9月 15 17:03   ← 早于 wg 特性提交 f47e58c(9月 15 17:09)
release/LocalProxy-0.3.0-arm64.app.zip    9月 17 14:35
release/mac-arm64/.../local_proxy_core    9月 23 11:51
运行中的 core: PID 73653, 启动于 9月 20 09:54（镜像来自 ≤9/17 的构建）
```

## 3. 代码层面的根因

### R1（确定）没有“单实例”语义：新 helper 先 unlink socket，再另起一个

`rust/src/core/wireguard.rs:520` `launch_helper()`：

```rust
let _ = std::fs::remove_file(&socket);      // 旧 helper 的 socket 文件被删掉
...
"do shell script \"pkill -f {proc} >/dev/null 2>&1 || true; {helper} --socket {sock} --token {token} >/dev/null 2>&1 &\" with administrator privileges"
```

- `pkill` 的失败被 `|| true` 吞掉，返回码从不校验；命中的前提是旧进程 **argv 仍然匹配**。
- 旧 socket 已被 unlink，旧 helper 既收不到 `Shutdown`（协议里唯一的优雅退出路径），也无法再被 `Status` 发现。
- helper 侧（`local_proxy_wireguard_helper.rs:103-109`）同样是 `remove_file` + `bind`，所以每次都会“抢走”socket 文件；**旧进程不会退出**，它会一直握着自己的 `DeviceHandle`（utun）直到被杀。
- 结果就是 §2.1 的 7 个共存进程和 §2.2 的地址堆积。

### R2（确定）清理不完整：`cleanup_tunnel` 从不拆除接口地址，路由按“目的地址”删除，DNS 无归属判断

`local_proxy_wireguard_helper.rs:579`：

```rust
fn cleanup_tunnel(tunnel: &ActiveTunnel) {
    for route in &tunnel.routes {
        let _ = run_command("/sbin/route", &["-n", "delete", route]);   // 只按目的地址删
    }
    if tunnel.dns_service.is_some() { restore_dns(tunnel); }            // 恢复的是“本 helper 记录的上一次 DNS”
}
```

- 没有 `ifconfig <utun> inet <addr> delete`，也没有任何“销毁设备”的步骤；`utun` + 地址只能靠进程退出被动回收，进程活着就永久残留。
- `route delete <dest>` 不校验这条路由当前是否还指向自己的接口：存在多个 tunnel 时，**后一个 helper 的清理会删掉当前 live 隧道的路由**。
- `restore_dns` 会把“自己启动前读到的 DNS”写回去：多个 helper 交错启动时，**旧 helper 的恢复动作会覆盖新 tunnel 的 DNS**。
- 另外 `Cargo.toml` 里 `ctrlc = "3"`（未开启 `termination` feature），helper 只对 SIGINT 注册了处理器，**SIGTERM 走默认动作**：被 `pkill` 杀掉时不会执行 `cleanup_tunnel` → 路由/DNS 残留（设备随进程消失）。

### R3（确定）350ms 的探活预算 + 单线程阻塞 helper ⇒ 误判“helper 过期” ⇒ 重新拉起 helper ⇒ 又要输密码

- `wireguard.rs:496` `helper_is_current()` 用 **350 ms** 超时 Ping；`wireguard.rs:487` `ensure_helper()` 认为不新鲜时会 `Shutdown`（500 ms）后直接 `pkill + osascript` 重启 helper。
- `wireguard.rs:396` `call_helper()` **每次调用都先 `ensure_helper()`**；而 `wireguard.rs:347-364` 的隧道守护循环 **每 5 秒** 调用一次 `call_helper(Status)`。也就是说：守护循环本身就是重启 helper 的触发源，而重启 helper = 一次 `with administrator privileges` = 一次密码弹窗。
- helper 是 `new_current_thread`（`local_proxy_wireguard_helper.rs:89-93`），却在同一线程里做阻塞活：`DeviceHandle::new`、`send_uapi`（2 s 读超时）、`ifconfig`、多条 `route add`、`networksetup -setdnsservers`（`setup_tunnel`，`:229-340`）。`Mutex<HashMap>` 还在这些阻塞调用期间被持有（`:191-201`）。Status/Setup 进行中它的 Ping 队列会排到 350 ms 之后 → 被判定为“helper 不新鲜” → 被 `pkill` 掉换一个新的。
- 更糟的是：**被误杀的是当时正在工作的那个 helper**。杀掉后新 helper 没有任何隧道，守护循环下一次 Status 拿到 `null` → `wireguard.rs:357` 直接 `return Err("WireGuard helper 已失去该隧道")`，隧道就此结束（UI 变成不可用），用户只能手动重启 → 又是一次密码。

### R4（确定）唯一的特权路径是 osascript 管理员授权，没有开机常驻的特权 helper

`wireguard.rs:533-545` 只有 `do shell script ... with administrator privileges` 一条路：Authorization Services 的票据只在短时间内有效，**锁屏 / 睡眠 / 用户切换会失效**。项目里没有 LaunchDaemon / `SMAppService` / XPC 特权助手，所以“重新拉起 helper”必然等于“重新输管理员密码”。

### R5（确定）没有锁屏/唤醒感知，也没有握手健康检查

- `electron/` 与 `src/` 中没有任何 `powerMonitor` / `unlock-screen` / `resume` 处理，`grep` 无命中：锁屏唤醒后没有人主动去重建隧道。
- 守护循环只确认“helper 还记得这个隧道”（`Status`），**不看 `last_handshake_time_sec` / rx / tx**；`tunnel_status()` 已经把这些字段解析出来了（`local_proxy_wireguard_helper.rs:588-657`）却没人用于健康判断。所以“WS/UDP 会话在睡眠后失效、wg 不再有握手”这种情况会表现为“界面运行中但网络不通”，直到用户手动重启。
- `wireguard.rs:431-435` 读取响应**没有读超时**（`status()` 也没有外层超时，只有 `call_helper` 有 120 s）。helper 一旦卡住，UI 的 5 秒轮询（`src/App.tsx:728-736`）会不断堆积悬挂请求，界面状态冻结。

### R6（假设，待验证）残留设备可能真的在“抢”同一个 server peer

多个残留 helper 各自持有 **相同私钥、相同地址 192.168.1.6/32** 的设备，且它们的 peer 配置（Endpoint 指向本地 wstunnel 端口，例如 `localhost:50814`）都还在生效。这些设备会从不同源端口向同一个 server 端点做握手/keepalive，服务端只有这一个 peer，会不断“跟随最新来源”漫游，从而出现 live 隧道时通时断的现象。这一条与 §2.3（路由只挂在 `utun9`）一致，但需要抓包/服务端日志确认。

### R7（开放）为什么 9/15 的 6 个 helper 在 9/24 的 `pkill` 之后仍然活着？

已完成的验证：

- 代码里的 `pkill -f` 模式确实能匹配这些进程（`pgrep -f "<pattern>"` 命中全部 7 个）。
- 用同样的 `osascript "do shell script ..."`（不加管理员权限、不会弹窗）在本机复现：**旧 helper 被 SIGTERM 杀掉、`osascript` 退出码 0、新 helper 正常拉起** —— 机制本身是通的（已被测试脚本验证）。
- 这些残留进程当前 `ps -o sigmask` 显示 BLOCKED=0，即 SIGTERM 没有被阻塞。

因此只剩两种可能，尚未区分：

1. **运行中的 App 镜像早于 pkill 修复**（PID 73653 启动于 9/20，磁盘上的 9/23 构建才肯定含该行；`release/LocalProxy-0.2.0-arm64.dmg` 的时间 9/15 17:03 早于提交 `f47e58c`）；
2. 这些由 **`with administrator privileges` 路径** 启动的 root 进程在信号处置上确有特殊之处，`pkill` 的 SIGTERM 无效。

判定命令（需要一次管理员密码）：

```bash
sudo kill -TERM 3583 3614 86436 86502 89142 95252; sleep 1
ps -o pid,stat,command -p 3583,3614,86436,86502,89142,95252
# 全部消失 => 原因 1（旧构建 / 未执行 pkill）
# 仍然存在 => 原因 2（管理员路径启动的进程不可被 SIGTERM 终止）
```

## 4. 立即处置（清掉本次现场残留）

本次现场的 6 个孤儿 helper（9/15 启动、协议 v2、没有 pid 文件）在**新 helper 第一次启动时会被自动收掉**（见 §5.5），所以正常情况下不需要手工清理。如果希望现在就清干净、或者想立刻验证，也可以手工执行：

```bash
# 0) 先在 App 里停掉该 WireGuard profile，避免边清边建
# 1) 一次管理员密码，收掉所有残留 helper
sudo pkill -f local_proxy_wireguard_helper; sleep 1
pgrep -fl local_proxy_wireguard_helper || echo "helper 已清空"

# 2) 设备应随进程退出自动消失；确认是否还有重复地址
ifconfig | grep -E '^utun|inet 192.168.1.6'
# 若某个 utun 仍残留地址（进程已死但设备还在）：
#   sudo ifconfig utun5 inet 192.168.1.6 delete

# 3) 清掉无人持有的 UAPI socket（新版 helper 启动时也会自动清扫）
sudo ls -la /var/run/wireguard/
#   sudo rm -f /var/run/wireguard/utun5.sock /var/run/wireguard/utun6.sock ...
```

注意：运行中的旧版本 App（PID 73653，镜像来自 9/17）仍然带着旧 helper 的 bug，最好一并替换成新构建后再观察。

## 5. 已落地的修复（2026-09-24）

### 5.1 单实例接管，取代 `remove_file` + `pkill -f`（修 R1、R7 的影响面）

`rust/src/helper_paths.rs`（新增）统一定义 socket / pid / lock 路径与 `HELPER_PROTOCOL_VERSION = 3`；
`rust/src/bin/local_proxy_wireguard_helper.rs` 的 `acquire_singleton()`：

- 启动先对 `<socket 目录>/localproxy-wireguard.lock` 取 `flock(LOCK_EX|LOCK_NB)`，拿到锁才是当前实例，并写入 `localproxy-wireguard.pid`；
- 拿不到锁时读取 pid 文件，**确认该 pid 仍是 `local_proxy_wireguard_helper` 进程**（`ps -o comm=`）后才 `SIGTERM`，3 秒宽限后 `SIGKILL`，再等锁；pid 被回收时绝不会误杀无关进程；
- 只有拿到锁之后才 `remove_file(socket)` + `bind`，所以“socket 文件被删掉、旧进程还在”这条孤儿路径被彻底切断；
- 核心侧 `launch_helper()` 不再删除 socket 文件，也不再拼 `pkill -f` 脚本，只做一次 `do shell script`；启动后必须 **Ping 到协议版本 3** 才算成功（旧版本 helper 会被判定为“不可复用”，在下次显式启动时替换）。

### 5.2 探活与重启策略：不再“误杀正在工作的 helper”（修 R3）

- `helper_is_current()` 改为 **3 次尝试 × 900 ms**（原来 1 次 × 350 ms），并且失败不再刷日志；
- `ensure_helper(mode)` 引入 `HelperMode`：`Explicit`（用户显式启动 profile，允许拉起 helper、允许弹密码）与 `Existing`（只复用已在运行的 helper，否则报错）。**5 秒守护循环、UI 轮询、唤醒检查全部走 `Existing`**，因此这些路径不可能再触发密码弹窗；
- `call_with_token()` 增加 30 秒读超时，`status()` 增加 4 秒外层超时（UI 轮询不再堆积悬挂请求）；
- 守护循环连续 4 次（约 20 秒）通信失败才结束隧道并给出明确错误。

### 5.3 完整拆除（修 R2）

`cleanup_tunnel()` 现在按“自己装了什么就拆什么”执行：

- 删除本设备配置过的接口地址：IPv4 `ifconfig <utun> inet <addr> delete`，IPv6 同理（以前从不删，只能靠进程退出回收）；
- 路由删除带上接口限定 `route -n delete -inet -host/-net <dest> -interface <utun>`，失败再用 `route -n get` 复核归属后才普通删除 —— 旧 helper 不会删掉新隧道的路由；
- 路由**添加**遇到冲突时直接报错，不删除其他接口的路由；仅凭 `route get` 无法确认它属于残留 helper，直接接管会中断其他 VPN 或用户配置；
- DNS 仅在“当前值仍等于本 helper 写入的值”时才恢复（`restore_dns_if_owned`），多个隧道交错启动时不会互相覆盖；
- `Setup` 失败时回滚已经施加的一半配置。

### 5.4 信号与运行时（修 R2 的一部分）

- `ctrlc` 开启 `termination` feature：SIGTERM / SIGHUP 与 SIGINT 一样走完整清理（`shutdown()` 拆隧道、删 pid 文件、删 socket）；
- helper 运行时由 `new_current_thread` 改为 **2 线程 multi-thread**，并且 `Setup / Stop / Status / Shutdown` 全部通过 `spawn_blocking` 执行；`Mutex<HashMap<String, ActiveTunnel>>` 只在取快照/摘除条目时短暂持有，**不再跨 `ifconfig`/`route`/`networksetup` 阻塞调用持有**。`Setup`、`Stop` 和 `Shutdown` 另有一把变更锁，保证拆建顺序；核心侧也把恢复与停止串行处理，并跳过正在启动或停止的 profile，避免停止期间重新插入隧道。每次 utun 创建使用独立的接口名文件。

### 5.5 启动收敛

helper 启动后做两件事：

1. 清扫 `/var/run/wireguard/*.sock`：只删除 **对应 utun 已不存在** 的 sock 文件（被 `SIGKILL` 的 helper 会留下它），存活接口（含其它工具的）不动；
2. `reap_orphan_helpers()` 收掉**同 socket 路径的旧协议 helper**：v3 之前没有 pid 文件，接管逻辑找不到它们，于是它们会一直握着自己的 utun。判定条件必须是「可执行文件确实是 `local_proxy_wireguard_helper`」且「argv 里带本 socket 路径」（`ps -o command=`），先 SIGTERM、2 秒后 SIGKILL。这正是本次现场那 6 个 9/15 孤儿的清理路径，而且不会再误伤无关进程。

### 5.6 唤醒 / 解锁后自愈（修 R5）

- `electron/main.ts` 订阅 `powerMonitor` 的 `resume` 与 `unlock-screen`，延迟 3 秒发送新的 RPC `wireguard_revalidate`；
- 核心 `core::wireguard_revalidate()`：遍历**当前活跃的 WireGuard profile**（或指定 id），用 `helper::revalidate()` 检查 `handshake_age_sec`；
  - 配置了 `PersistentKeepalive` 的 peer 有近期握手，或未配置 keepalive 的隧道处于正常空闲状态 → 什么都不做；
  - 不健康 / helper 已丢失该隧道 → 通过**已运行的 helper** 重新 `Setup`（不再弹密码）；
- 守护循环只对配置了 `PersistentKeepalive` 的 peer 按握手新鲜度判断；连续 3 个周期（15 秒）没有新握手、且隧道已运行超过 30 秒时，自动重新下发配置，带 60 秒冷却、最多 5 次，达到上限后写日志提示手动重启；
- 隧道状态为 `present` 但 helper 丢失隧道时，守护循环会先尝试用 `Existing` 模式重新下发配置，而不是立刻把隧道判死。

### 5.7 结构化记录（直接对应“感觉需要记录”）

日志文件：`~/Library/Application Support/LocalProxy/wireguard-helper.log`（超过 1 MiB 轮转为 `wireguard-helper.log.1`）。

```text
2026-09-24T15:11:03Z [launch] 请求启动 helper（替换 pid=无）
2026-09-24T15:11:08Z [launch] helper 就绪 pid=30228（替换 pid=无）
2026-09-24T15:11:09Z [tunnel] 3f2c…: 已就绪 utun9 → 1 peer(s): localhost:50814
2026-09-24T15:24:10Z [health] 3f2c…: utun9 上握手信息过旧（从未完成握手），重新下发配置（第 1 次）
2026-09-24T15:25:11Z [probe]  3f2c…: helper 状态查询失败（第 1 次）: WireGuard helper 未在运行…
2026-09-24T15:26:02Z [tunnel] 3f2c…: 已停止
```

### 5.8 helper 侧请求校验（纵深防御，2026-09-24 追加）

helper 是 root 进程，而它的 socket 与 token 都位于用户会话可读的位置（同一个用户下的其它进程可以拿到 token 并直接对话 root helper）。因此不再只依赖核心侧校验：

- `validate_config()` 在 `Setup` 入口复核 Address（1–8 个、CIDR 合法、不重复）、DNS（≤8 个、非空、不以 `-` 开头、无空格/引号/反引号/`;` 等，避免被 `networksetup` 当成选项）、PrivateKey/PublicKey（32 字节 Base64）、Peer 数量与 Endpoint、AllowedIPs（1–64 个、前缀合法），并**拒绝 `0.0.0.0/0` 与 `::/0`**（核心本来就拒绝，这里再挡一次，防止 helper 被直接调用时接管全部流量）。

## 6. 验证

```bash
cargo test --manifest-path rust/Cargo.toml          # 41 个测试，含新增的 11 个
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings
cargo build --release --manifest-path rust/Cargo.toml
npm run lint && npm run build:electron
```

新增的回归测试覆盖（helper 侧校验、单实例锁、路由/DNS 归属、健康阈值等）：单实例锁互斥、pid 文件里是死进程时仍能接管、锁被占用时给出“单实例”错误、路由 family/kind 推导、地址族映射、DNS 归属比较（忽略顺序与大小写）、健康判定阈值、时间戳格式、helper 与核心的 pid/lock 路径一致。

手工验证（建议顺序）：

1. 启动一个 WireGuard profile，确认一次密码后可正常连通；
2. 锁屏 → 解锁：观察日志是否出现 `[health]` 重新下发，且**不再弹密码**；
3. 反复停止/启动同一 profile 5 次，`pgrep -fl local_proxy_wireguard_helper | wc -l` 应恒为 1（macOS 的 pgrep 没有 `-c`），`ifconfig | grep -c 192.168.1.6` 不应增长；
4. `sudo kill -TERM $(cat $TMPDIR/localproxy-wireguard/localproxy-wireguard.pid)`：确认 `netstat -rn` 里该隧道的路由与 `networksetup -getdnsservers` 的 DNS 都已回退。

## 7. 仍未解决 / 可选的后续工作

按“收益 ÷ 成本”排序：

1. **P2：`SMAppService` + LaunchDaemon/XPC**（根治密码弹窗）。当前除了“首次启动 profile”之外都不再触发授权，但 helper 与 App 生命周期仍不一致：helper 会常驻到系统重启，且彻底退出后下一次启动仍要一次密码。挂到 launchd 后这两点都消失。
2. **恢复路径不要重建设备**：现在 `Setup` 一律「拆掉旧 tunnel → 新建 utun → 重配地址/路由/DNS」。锁屏自愈时其实可以只重发一次 UAPI `set=1`（重新武装 peer、触发新的握手），只有配置真的变了或第二次仍不健康时才重建 —— 能避免自愈本身造成的接口抖动与 DNS/路由 churn。
3. **自愈次数上限后的行为**：每个 tunnel 生命周期最多自动恢复 5 次，之后只写日志（唤醒路径仍会单独尝试）。可以改成“之后每 5 分钟静默重试一次”，或者把状态上报到 UI 让用户点一下。
4. **承载隧道不在自愈范围内**：示例里 WireGuard 的 Endpoint 是 `localhost:50814`，真正的传输是 wstunnel（或 SSH）。如果那一条挂了，wg 侧无论重下发多少次都不会有握手。建议在 “连续 N 次恢复失败” 时就检查该 profile 依赖的本地端口是否仍在监听，并在 UI/日志里明确提示“请检查承载隧道”，而不是只报 wg 不健康。
5. **UI 的瞬时假离线**：`wireguard_status` 现在有 4 秒超时，helper 忙时轮询失败会把 `wgStatus` 置为 `null`（`src/App.tsx` 的 catch）。可以保留上一次状态并标记「数据可能过期」。顺带一提，peer 的握手/rx/tx 已经在详情面板里显示了（`handshake_age_sec` / `rx_bytes` / `tx_bytes`），只差一个整体健康度提示。
6. **日志可再补上下文**：现在记了 `id → utunN`、暂停/恢复、探测失败；可以再把每条路由/DNS 的施加与回滚结果写进去，便于复盘这一类“归属判断”问题。
7. **R7 的最终原因未定**：新版不再依赖 `pkill -f`，已不阻塞；如需复盘历史现场，仍要一次 `sudo kill -TERM <pids>` 来区分“旧构建未执行 pkill”与“管理员路径进程不吃 SIGTERM”。
8. **R6（多个设备抢同一 peer）**：单实例后同一时刻只有一个设备持有该地址，风险自然消失；如果以后允许同一份 wg 配置被多个 profile 复用，需要重新评估。
9. **helper 空闲退出策略**：目前 helper 会一直常驻（这是“不再反复要密码”的代价）。可考虑给用户一个开关：退出 App 时一并结束 helper（更干净、但要重新授权），或保留常驻（现状）。
