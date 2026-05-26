//! 性格台词池 + 触发器。
//!
//! 在关键事件（开局加注 / 摊牌赢输 / 进入 tilt 等）发生时，
//! 按性格随机抽一句中文口语写入 log，让 bot "活起来"。
//! 设计为 *偶发*：每次 ~50% 概率返回 None，避免刷屏。

use rand::Rng;

use crate::bot::personality::Personality;

/// 触发台词的事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceEvent {
    /// 自己开局 raise（preflop 主动加注）。
    OpenRaise,
    /// 自己摊牌赢。
    WonShowdown,
    /// 自己摊牌输。
    LostShowdown,
    /// 自己被无对抗收下底池（其他人都 fold）。
    WonUncontested,
    /// 自己进入 tilt 状态。
    TiltOn,
    /// 自己弃牌。
    Folded,
    /// 自己全押。
    AllIn,
}

/// 按性格 × 事件抽一句台词；返回 None 表示 "这次不开口"。
pub fn pick_line<R: Rng + ?Sized>(
    persona: &Personality,
    ev: VoiceEvent,
    rng: &mut R,
) -> Option<&'static str> {
    let lines = lines_for(persona, ev);
    if lines.is_empty() {
        return None;
    }
    // 每次 50% 概率开口，避免每个事件都说话。
    if rng.r#gen::<f64>() > 0.5 {
        return None;
    }
    let idx = rng.r#gen::<usize>() % lines.len();
    Some(lines[idx])
}

/// 强制返回一句（用于测试），不参与"50% 概率"过滤。
pub fn lines_for(persona: &Personality, ev: VoiceEvent) -> &'static [&'static str] {
    use VoiceEvent::*;
    match (persona.label, ev) {
        // ===== Rock 紧弱 =====
        ("Rock", OpenRaise) => &["来真的了", "这把不开玩笑", "稳稳的"],
        ("Rock", WonShowdown) => &["运气好", "意料之中", "稳"],
        ("Rock", LostShowdown) => &["……奇怪", "概率问题", "下把再看"],
        ("Rock", WonUncontested) => &["好", "接着来"],
        ("Rock", TiltOn) => &["不对劲了", "今晚怎么回事"],
        ("Rock", Folded) => &["放着", "不掺和"],
        ("Rock", AllIn) => &["搏一把", "梭了"],

        // ===== Shark 紧凶 =====
        ("Shark", OpenRaise) => &["压一下", "看你跟不跟", "节奏给我"],
        ("Shark", WonShowdown) => &["读得很清楚", "教你做人", "意料之中"],
        ("Shark", LostShowdown) => &["运气", "下手就懂了", "嗯。"],
        ("Shark", WonUncontested) => &["明智", "好，下一手"],
        ("Shark", TiltOn) => &["……心态稍调一下"],
        ("Shark", Folded) => &["让你", "等更好的"],
        ("Shark", AllIn) => &["上限", "决一下"],

        // ===== Maniac 松凶 =====
        ("Maniac", OpenRaise) => &["上！", "都跟不跟？", "怕什么呢", "这把我做主"],
        ("Maniac", WonShowdown) => &["哈哈哈", "都说了！", "随便玩玩"],
        ("Maniac", LostShowdown) => &["艹", "再来再来", "这能输？"],
        ("Maniac", WonUncontested) => &["怂啥", "送钱呗", "hhh"],
        ("Maniac", TiltOn) => &["我急了我急了", "干他！"],
        ("Maniac", Folded) => &["勉强忍一下", "下手干"],
        ("Maniac", AllIn) => &["梭！", "全压！", "看天意"],

        // ===== Fish 松弱 =====
        ("Fish", OpenRaise) => &["试一下", "我觉得不错", "凑个热闹"],
        ("Fish", WonShowdown) => &["啊？我赢了？", "运气！", "嘿嘿"],
        ("Fish", LostShowdown) => &["差一点啊…", "下把肯定中", "我牌挺好啊"],
        ("Fish", WonUncontested) => &["哎呀这就赢了", "都让我？"],
        ("Fish", TiltOn) => &["我牌都那么烂的吗", "不服"],
        ("Fish", Folded) => &["勉强扔了", "心疼"],
        ("Fish", AllIn) => &["梭了梭了", "信一把"],

        // ===== Trapper 钓鱼者 =====
        ("Trapper", OpenRaise) => &["小试一下", "随便加点", "看你反应"],
        ("Trapper", WonShowdown) => &["哎呀，让你了", "嘿嘿，正好", "运气而已"],
        ("Trapper", LostShowdown) => &["……可惜", "差一线", "嗯，记下了"],
        ("Trapper", WonUncontested) => &["谢谢配合", "嗯哼"],
        ("Trapper", TiltOn) => &["这……节奏乱了"],
        ("Trapper", Folded) => &["躲一下", "等好牌"],
        ("Trapper", AllIn) => &["收网", "时候到了"],

        // ===== Bluffer 演员 =====
        ("Bluffer", OpenRaise) => &["这把我有", "信不信？", "别犹豫", "戏来了"],
        ("Bluffer", WonShowdown) => &["演技在线", "看到没，真的有", "请买票"],
        ("Bluffer", LostShowdown) => &["翻车了", "演砸了哈哈", "下回找回来"],
        ("Bluffer", WonUncontested) => &["怂了吧", "心理战", "嘿嘿嘿"],
        ("Bluffer", TiltOn) => &["不行，得拉回来", "状态调整"],
        ("Bluffer", Folded) => &["演不下去", "保留实力"],
        ("Bluffer", AllIn) => &["all in 你跟不跟", "压上戏份", "谁怕谁"],

        // ===== Nit 超紧 =====
        ("Nit", OpenRaise) => &["范围很窄", "这手可以", "难得参与"],
        ("Nit", WonShowdown) => &["标准结果", "只打好牌", "没什么意外"],
        ("Nit", LostShowdown) => &["这也能输", "样本太小", "下次更紧"],
        ("Nit", WonUncontested) => &["正常", "弃得对"],
        ("Nit", TiltOn) => &["我要再收紧一点", "风险超标"],
        ("Nit", Folded) => &["不够好", "继续等"],
        ("Nit", AllIn) => &["没退路了", "只有这手"],

        // ===== Station 跟注站 =====
        ("Station", OpenRaise) => &["偶尔来一下", "我也加一次"],
        ("Station", WonShowdown) => &["看吧，跟到了", "我就说能中", "不信邪有用"],
        ("Station", LostShowdown) => &["再看一张就好了", "我以为你没有", "跟得有点远"],
        ("Station", WonUncontested) => &["这就不跟了？", "我还想看牌呢"],
        ("Station", TiltOn) => &["我今天必须看河牌", "别想吓我"],
        ("Station", Folded) => &["这次真算了", "太贵了"],
        ("Station", AllIn) => &["我跟到底", "摊牌吧"],

        // ===== Pro 职业型 =====
        ("Pro", OpenRaise) => &["标准开局", "位置不错", "压力给上"],
        ("Pro", WonShowdown) => &["范围判断没错", "价值拿满", "结果合理"],
        ("Pro", LostShowdown) => &["线没问题", "下次调整", "记下频率"],
        ("Pro", WonUncontested) => &["弃牌率够高", "拿下"],
        ("Pro", TiltOn) => &["保持纪律", "不被结果影响"],
        ("Pro", Folded) => &["这手到此为止", "范围不支持"],
        ("Pro", AllIn) => &["最大压力", "到决策点了"],

        // ===== Gambler 赌徒 =====
        ("Gambler", OpenRaise) => &["今天手气不错", "翻倍机会", "敢不敢一起"],
        ("Gambler", WonShowdown) => &["赌对了", "手气也是实力", "这就是命"],
        ("Gambler", LostShowdown) => &["差一点翻倍", "再来一把", "牌运不站我这边"],
        ("Gambler", WonUncontested) => &["不敢赌啊", "收了收了"],
        ("Gambler", TiltOn) => &["下一把翻回来", "我要追一下"],
        ("Gambler", Folded) => &["这把不押", "等个大机会"],
        ("Gambler", AllIn) => &["一把定输赢", "翻倍就在现在"],

        // ===== WeakTight 紧弱 =====
        ("WeakTight", OpenRaise) => &["应该能打吧", "小心一点加"],
        ("WeakTight", WonShowdown) => &["还好还好", "幸亏没弃", "稳住了"],
        ("WeakTight", LostShowdown) => &["早知道弃了", "太危险了", "我不该跟"],
        ("WeakTight", WonUncontested) => &["没人跟最好", "安全拿下"],
        ("WeakTight", TiltOn) => &["我得冷静", "不能再乱跟了"],
        ("WeakTight", Folded) => &["算了", "不冒险"],
        ("WeakTight", AllIn) => &["只能这样了", "有点慌"],

        // ===== BalancedReg 均衡常规玩家 =====
        ("BalancedReg", OpenRaise) => &["标准开池", "范围覆盖一下", "位置可以"],
        ("BalancedReg", WonShowdown) => &["执行到位", "赔率合适", "这手合理"],
        ("BalancedReg", LostShowdown) => &["样本而已", "线可以优化", "下次调整频率"],
        ("BalancedReg", WonUncontested) => &["弃牌率够", "拿下小池"],
        ("BalancedReg", TiltOn) => &["不要偏离策略", "继续按范围来"],
        ("BalancedReg", Folded) => &["范围外", "弃牌没问题"],
        ("BalancedReg", AllIn) => &["权益够了", "压力给满"],

        // ===== ShortStacker 短码推压 =====
        ("ShortStacker", OpenRaise) => &["筹码浅，简单点", "直接给压力"],
        ("ShortStacker", WonShowdown) => &["翻倍成功", "短码也能打", "刚好够用"],
        ("ShortStacker", LostShowdown) => &["短码命", "没转起来", "下次推准点"],
        ("ShortStacker", WonUncontested) => &["偷到就行", "盲注也是钱"],
        ("ShortStacker", TiltOn) => &["再找个翻倍点", "不能等死"],
        ("ShortStacker", Folded) => &["不是推点", "再等一圈"],
        ("ShortStacker", AllIn) => &["推了", "十盲不等人"],

        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn each_persona_event_has_lines() {
        use VoiceEvent::*;
        let events = [
            OpenRaise,
            WonShowdown,
            LostShowdown,
            WonUncontested,
            TiltOn,
            Folded,
            AllIn,
        ];
        for p in Personality::PRESETS.iter() {
            for ev in events {
                let lines = lines_for(p, ev);
                assert!(!lines.is_empty(), "{} × {:?} has no lines", p.label, ev);
                for l in lines {
                    assert!(!l.is_empty());
                }
            }
        }
    }

    #[test]
    fn pick_line_eventually_returns_some() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut got_some = false;
        for _ in 0..50 {
            if pick_line(&Personality::MANIAC, VoiceEvent::OpenRaise, &mut rng).is_some() {
                got_some = true;
                break;
            }
        }
        assert!(got_some, "pick_line never returned Some over 50 trials");
    }
}
