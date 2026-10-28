//! High-Precision 2S Li-ion Battery Discharge Curves & Piecewise Interpolation.

/// Battery chemistry / capacity type profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryType {
    Type1600mAh = 0,
    Type2200mAh = 1,
    Type3500mAh = 2,
    Type1500mAhK1 = 3,
    Type2500mAhK1 = 4,
    TypeExternal = 5,
}

impl BatteryType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => BatteryType::Type1600mAh,
            1 => BatteryType::Type2200mAh,
            2 => BatteryType::Type3500mAh,
            3 => BatteryType::Type1500mAhK1,
            4 => BatteryType::Type2500mAhK1,
            5 => BatteryType::TypeExternal,
            _ => BatteryType::Type1600mAh,
        }
    }
}

/// High-precision 2S Li-ion discharge curves (10mV resolution units -> Percentage 0..100)
/// Modeled after industry 2S Li-ion cell characteristics (8.4V max, 7.4V nominal, 6.0V cutoff).
pub const VOLTAGE_CURVES: [&[(u16, u8)]; 5] = [
    // 1600 mAh Standard Pack (UV-K5)
    &[
        (838, 100),
        (820, 95),
        (780, 75),
        (745, 45),
        (715, 20),
        (670, 7),
        (600, 0),
    ],
    // 2200 mAh Extended Pack
    &[
        (839, 100),
        (825, 96),
        (785, 78),
        (750, 50),
        (720, 22),
        (680, 8),
        (600, 0),
    ],
    // 3500 mAh High Capacity Pack
    &[
        (840, 100),
        (828, 97),
        (790, 80),
        (755, 55),
        (725, 25),
        (685, 9),
        (600, 0),
    ],
    // 1500 mAh Compact Pack (K1)
    &[
        (836, 100),
        (818, 94),
        (778, 72),
        (742, 42),
        (712, 18),
        (668, 6),
        (600, 0),
    ],
    // 2500 mAh Extended Pack (K1)
    &[
        (839, 100),
        (826, 96),
        (788, 79),
        (752, 52),
        (722, 23),
        (682, 8),
        (600, 0),
    ],
];

/// High-precision piecewise linear interpolation to convert OCV battery millivolts to percentage.
pub fn mv_to_percentage(mv: u16, bat_type: BatteryType) -> u8 {
    if bat_type == BatteryType::TypeExternal {
        return 100;
    }
    let v_10mv = mv / 10;
    let curve = VOLTAGE_CURVES[bat_type as usize];

    if v_10mv >= curve[0].0 {
        return 100;
    }

    for i in 1..curve.len() {
        let (v_prev, p_prev) = curve[i - 1];
        let (v_curr, p_curr) = curve[i];

        if v_10mv >= v_curr {
            let v_range = (v_prev - v_curr) as u32;
            if v_range == 0 {
                return p_curr;
            }
            let p_range = (p_prev - p_curr) as u32;
            let v_diff = (v_10mv - v_curr) as u32;
            let interpolated = (p_curr as u32) + (v_diff * p_range) / v_range;
            return (interpolated.min(100)) as u8;
        }
    }

    0
}
