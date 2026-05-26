//! 性格 → 个性化中文绰号采样。
//!
//! 每个性格挂一组绰号，开局时为每个 bot 随机抽一个；
//! 若同桌冲突则在该性格池里继续抽，仍冲突再 fallback 到 "name#N"。

use rand::Rng;
use rand::seq::SliceRandom;

use crate::bot::personality::Personality;

/// 返回该性格的中文绰号池。
pub fn name_pool(persona: &Personality) -> &'static [&'static str] {
    match persona.label {
        "Rock" => &["老石头", "龟仙人", "巴菲特", "稳如山", "老干部"],
        "Shark" => &["阿鲨", "扑克教练", "老K", "冷面手", "教授"],
        "Maniac" => &["上头哥", "李疯子", "电锯", "刹不住", "炸药包"],
        "Fish" => &["小白", "凯子", "阿弟", "海底捞", "活宝"],
        "Trapper" => &["老猫", "钓王", "黑寡妇", "深水", "藏锋"],
        "Bluffer" => &["影帝", "大忽悠", "美猴王", "戏精", "嘴炮王"],
        "Nit" => &["铁闸", "保险柜", "老锁", "风控官", "只打AA"],
        "Station" => &["跟到底", "呼叫台", "不信邪", "河边等", "粘人精"],
        "Pro" => &["牌桌经理", "冷读者", "算子", "终局者", "职业哥"],
        "Gambler" => &["赌徒", "硬币哥", "手气王", "搏命仔", "翻倍侠"],
        "WeakTight" => &["怕输哥", "小心眼", "安全带", "缩手王", "保本派"],
        "BalancedReg" => &["常规哥", "平衡师", "牌局工兵", "稳准狠", "标准答案"],
        "ShortStacker" => &["短码侠", "推土机", "浅筹码", "翻倍党", "十盲王"],
        _ => &["路人甲"],
    }
}

/// 为一桌 bots 采样不重复的绰号。返回长度等于 personas.len() 的 Vec。
pub fn sample_table_names<R: Rng + ?Sized>(personas: &[Personality], rng: &mut R) -> Vec<String> {
    let mut used: Vec<String> = Vec::with_capacity(personas.len());
    let mut out: Vec<String> = Vec::with_capacity(personas.len());

    for p in personas {
        let pool = name_pool(p);
        let mut shuffled: Vec<&'static str> = pool.to_vec();
        shuffled.shuffle(rng);

        let mut picked: Option<String> = None;
        for n in &shuffled {
            if !used.iter().any(|u| u == n) {
                picked = Some(n.to_string());
                break;
            }
        }
        let name = picked.unwrap_or_else(|| {
            // fallback：池子里全冲突，加序号
            format!("{}#{}", shuffled[0], used.len())
        });
        used.push(name.clone());
        out.push(name);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn pool_nonempty_for_each_preset() {
        for p in Personality::PRESETS.iter() {
            assert!(!name_pool(p).is_empty(), "pool empty for {}", p.label);
        }
    }

    #[test]
    fn table_names_unique() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let names = sample_table_names(&Personality::PRESETS, &mut rng);
        assert_eq!(names.len(), Personality::PRESETS.len());
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            Personality::PRESETS.len(),
            "names not unique: {:?}",
            names
        );
    }

    #[test]
    fn duplicate_personas_still_unique_names() {
        // 同性格 3 个，应仍能从池子里抽出 3 个不同的
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let personas = vec![Personality::ROCK; 3];
        let names = sample_table_names(&personas, &mut rng);
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 3);
    }
}
