# poker — 命令行德州扑克 (人 vs AI Bots)

一个 Rust 写的 No-Limit Texas Hold'em 现金桌：你坐 0 号位，对面 1–6 个 AI bot
轮流坐庄。每个 bot 有不同的 *打牌风格*，靠蒙特卡洛胜率 + 性格档案做出决策。

## 运行

```bash
cargo run --release -- --bots 5
# 或者指定性格：
cargo run --release -- --personalities "Rock,Shark,Maniac,Trapper,Bluffer"
```

按键：

| 键 | 动作 |
|----|------|
| `F` | Fold |
| `K` | Check (无人下注时)|
| `C` | Call (有人下注时) |
| `R` | Raise；输入金额后回车 |
| `A` | All-in |
| `Esc` / `Q` | 退出 |
| `Space` / `Enter` | 进入下一手 |
| `R` (手与手之间) | 回放上一手 |

## 命令行参数

```
--bots <N>            1..=6，默认 5
--personalities <s>   逗号分隔，如 "Shark,Fish"
--stack <N>           初始筹码，默认 1500
--sb / --bb           盲注，默认 5 / 10
--hands <N>           手数上限；0 = 无限
--name <s>            真人显示名
--quiet               不显示 bot 台词
--no-anim             关闭发牌 / 摊牌动画
--replay-keep <N>     回放历史保留手数（默认 10，0 = 关闭）
```

## 6 种 Bot 风格 + 个性化绰号

每位 bot 启动时随机抽一个中文绰号，保证同桌不重复：

| 性格 | 风格 | VPIP | PFR | Aggr | Bluff | Slowplay | Show | 中文绰号池 |
|------|------|------|-----|------|-------|----------|------|-----------|
| **Rock** | 紧弱 | 14% | 8% | 0.8 | 2% | 5% | 2% | 老石头 / 龟仙人 / 巴菲特 / 稳如山 / 老干部 |
| **Shark** | 紧凶 (TAG) | 22% | 18% | 2.5 | 12% | 15% | 5% | 阿鲨 / 扑克教练 / 老K / 冷面手 / 教授 |
| **Maniac** | 松凶 (LAG) | 42% | 35% | 4.0 | 30% | 5% | 20% | 上头哥 / 李疯子 / 电锯 / 刹不住 / 炸药包 |
| **Fish** | 松弱 | 55% | 8% | 0.5 | 3% | 10% | 10% | 小白 / 凯子 / 阿弟 / 海底捞 / 活宝 |
| **Trapper** | 钓鱼者 | 20% | 10% | 1.5 | 8% | 40% | 8% | 老猫 / 钓王 / 黑寡妇 / 深水 / 藏锋 |
| **Bluffer** | 演员 | 30% | 25% | 3.0 | 45% | 10% | 25% | 影帝 / 大忽悠 / 美猴王 / 戏精 / 嘴炮王 |

## 戏剧化系统 (v0.2)

- **中文台词**：bot 在 `OpenRaise / Fold / AllIn / WonShowdown / LostShowdown / TiltOn` 等事件下随机开口（每次约 50% 概率），写入 log。`--quiet` 关闭。
- **Tilt 系统**：连续亏损 ≥3 手开始累积 tilt，让 effective `aggression / vpip / bluff_freq` 临时上调；UI 行尾显示 `[T]` 标记。赢一手后 tilt 衰减。`tilt_factor` 来自 Personality。
- **Show one card**：bot 弃牌时按 `show_freq` 概率秀一张底牌；render 在 fold 行显示 `秀: [A♠]`。
- **发牌动画**：进入 Flop / Turn / River 时逐张揭示（默认 220ms 一张），`--no-anim` 关闭。
- **赢家高亮**：摊牌后赢家行绿色显示 700ms。
- **底池金色**：pot ≥ 50 × bb 时显示为金色，提示大底池。
- **回放系统**：每手结束在 `Press <space> next, [R] replay` 提示按 R 进入回放，逐帧（350ms / 帧）重演刚才那手；按 q 中止。默认保留最近 10 手历史。

## 项目结构

```
src/
├── card.rs        # Rank/Suit/Card/Deck (52 张, u8 编码)
├── hand_eval.rs   # 5/7 张牌型评估 + Ord 比较
├── equity.rs      # MC 胜率 + preflop 启发式打分
├── game/
│   ├── action.rs    # Fold/Check/Call/Bet/Raise/AllIn
│   ├── state.rs     # HandState/Player/Stage
│   ├── betting.rs   # 发牌/贴盲/动作合法性/轮转/街切换
│   ├── showdown.rs  # 边池切分 + 摊牌结算
│   └── history.rs   # 手牌快照 + 回放数据
├── bot/
│   ├── personality.rs # 6 种预置性格档案
│   ├── decision.rs    # equity + pot_odds + 性格 + mood → Action
│   ├── mood.rs        # tilt 跟踪
│   ├── names.rs       # 性格 → 中文绰号采样
│   └── voice.rs       # 性格 × 事件 → 中文台词
├── ui/
│   ├── render.rs    # crossterm 全屏重绘 + 赢家/tilt 高亮
│   ├── anim.rs      # 发牌 / 摊牌动画
│   └── input.rs     # 真人键盘输入
├── config.rs      # CLI 参数
├── lib.rs / main.rs
tests/
├── integration.rs # bot vs bot 30 手 + 脚本真人
benches/
└── equity_bench.rs
```

## AI 决策骨架

Bot 每次行动时：

1. **Equity** — preflop 用 169-hand 启发式打分；postflop 跑 600 次蒙特卡洛
   抽样所有未知牌，统计胜+平/2 / N。
2. **Pot odds** = `to_call / (pot + to_call)` → 跟注需要的最低胜率。
3. **Edge** = `equity − pot_odds`：决定 fold / call / raise / bluff。
4. **性格调制**：
   - VPIP/PFR 阈值控制入池与开局加注；
   - aggression 影响 raise/call 的概率与下注大小；
   - bluff_freq 在弱牌 + 可怕板面下偶发诈唬；
   - slowplay_freq 让强牌偶尔不加；
   - position_aware：晚位 (button 附近) 阈值放宽。

## 测试 / 基准

```bash
cargo test                  # 35 单元测试 + 2 集成测试
cargo bench                 # MC equity 基准 (criterion)
```

集成测试包含：6 bot 跑 30 手筹码守恒、脚本真人始终 fold 跑通完整一手。

## 路线图

- v0.1: 核心引擎 + 6 性格 + CLI TUI （当前）
- v0.2: 对手行为画像（持续记录 VPIP/PFR）→ 更精细决策
- v0.3: 锦标赛模式（盲注递增 + 出局）
- v0.4: ratatui 改进 UI + 历史回放
