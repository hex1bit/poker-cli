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
                assert!(
                    !lines.is_empty(),
                    "{} × {:?} has no lines",
                    p.label,
                    ev
                );
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
