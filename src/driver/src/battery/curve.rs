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
pub struct CurveDefinition {
    pub v_max: u16,
    pub segments: &'static [(u16, u8, u32)],
}

pub const VOLTAGE_CURVES: [CurveDefinition; 5] = [
    // 1600 mAh Standard Pack (UV-K5)
    CurveDefinition {
        v_max: 838,
        segments: &[
            (820, 95, 18204),
            (780, 75, 32768),
            (745, 45, 56174),
            (715, 20, 54613),
            (670, 7, 18933),
            (600, 0, 6554),
        ],
    },
    // 2200 mAh Extended Pack
    CurveDefinition {
        v_max: 839,
        segments: &[
            (825, 96, 18725),
            (785, 78, 29491),
            (750, 50, 52429),
            (720, 22, 61167),
            (680, 8, 22938),
            (600, 0, 6554),
        ],
    },
    // 3500 mAh High Capacity Pack
    CurveDefinition {
        v_max: 840,
        segments: &[
            (828, 97, 16384),
            (790, 80, 29319),
            (755, 55, 46811),
            (725, 25, 65536),
            (685, 9, 26214),
            (600, 0, 6939),
        ],
    },
    // 1500 mAh Compact Pack (K1)
    CurveDefinition {
        v_max: 836,
        segments: &[
            (818, 94, 21845),
            (778, 72, 36045),
            (742, 42, 54613),
            (712, 18, 52429),
            (668, 6, 17873),
            (600, 0, 5783),
        ],
    },
    // 2500 mAh Extended Pack (K1)
    CurveDefinition {
        v_max: 839,
        segments: &[
            (826, 96, 20165),
            (788, 79, 29319),
            (752, 52, 49152),
            (722, 23, 63351),
            (682, 8, 24576),
            (600, 0, 6394),
        ],
    },
];

/// High-precision piecewise linear interpolation to convert OCV battery millivolts to percentage.
pub fn mv_to_percentage(mv: u16, bat_type: BatteryType) -> u8 {
    if bat_type == BatteryType::TypeExternal {
        return 100;
    }
    // Fast reciprocal division: div10_u16
    let (v_10mv, _) = utils::math::div10_u16(mv);
    let curve = &VOLTAGE_CURVES[bat_type as usize];

    if v_10mv >= curve.v_max {
        return 100;
    }

    for &(v_curr, p_curr, slope_q16) in curve.segments {
        if v_10mv >= v_curr {
            let v_diff = (v_10mv - v_curr) as u32;
            let interpolated = (p_curr as u32) + ((v_diff * slope_q16) >> 16);
            return (interpolated.min(100)) as u8;
        }
    }

    0
}
