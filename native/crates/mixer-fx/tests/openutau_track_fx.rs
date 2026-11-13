use domain::UMixFx;
use mixer_fx::OpenUtauTrackFx;

fn test_fx() -> UMixFx {
    let mut fx = UMixFx::default();
    fx.enabled = true;
    fx.eq_preset = "vocal_air".into();
    fx.eq_low_db = -1.5;
    fx.eq_mid_freq = 2400.0;
    fx.eq_mid_db = 2.0;
    fx.eq_high_db = 1.25;
    fx.comp_preset = "gentle".into();
    fx.comp_threshold_db = -18.0;
    fx.comp_ratio = 2.0;
    fx.comp_makeup_db = 2.5;
    fx.reverb_preset = "small_room".into();
    fx.reverb_size = 0.3;
    fx.reverb_damp = 0.7;
    fx.reverb_wet = 1.0;
    fx.reverb_pre_delay_ms = 12.0;
    fx
}

fn source(frames: usize) -> Vec<f32> {
    (0..frames * 2)
        .map(|index| {
            let t = index as f32 * 0.000_37;
            (t.sin() * 0.17 + (t * 0.31).cos() * 0.09) * if index % 177 == 0 { 1.4 } else { 1.0 }
        })
        .collect()
}

#[test]
fn track_fx_state_is_identical_when_processed_as_contiguous_blocks() {
    let input = source(160_000);
    let mut whole = input.clone();
    OpenUtauTrackFx::new(&test_fx()).process(&mut whole);

    let mut blocked = input;
    let mut fx = OpenUtauTrackFx::new(&test_fx());
    // 4096 stereo frames per call matches the maximum T033 stream block.
    for block in blocked.chunks_mut(4096 * 2) {
        fx.process(block);
    }

    assert_eq!(
        blocked, whole,
        "per-track FX must retain state across stream blocks"
    );
    assert_ne!(
        whole,
        source(160_000),
        "enabled EQ/compressor/reverb must affect the signal"
    );
}

#[test]
fn disabled_track_fx_is_transparent() {
    let input = source(4096);
    let mut output = input.clone();
    let mut fx = test_fx();
    fx.enabled = false;
    OpenUtauTrackFx::new(&fx).process(&mut output);
    assert_eq!(output, input);
}

#[test]
fn openutau_csharp_reference_vectors_match_track_fx() {
    // Captured by running OpenUtau.Core's `MixFxSource.WrapWith` on this exact
    // deterministic 160,000-frame stereo input with `test_fx()` parameters.
    // The C# and Rust arithmetic paths can differ by a float ULP, so compare
    // representative points with a tight absolute tolerance rather than
    // claiming bit-identical full-buffer PCM.
    let mut output: Vec<f32> = (0..320_000)
        .map(|i| (((i * 31) % 257) as i32 - 128) as f32 / 256.0)
        .collect();
    OpenUtauTrackFx::new(&test_fx()).process(&mut output);

    let reference = [
        (0, -0.6460657716_f32),
        (1, -0.4895966947),
        (100, -0.5504769683),
        (1_000, 0.1124046817),
        (1_058, 0.1084841788),
        (20_000, -0.02730009332),
        (100_000, -0.1295696795),
        (200_000, 0.02562780678),
        (319_998, 0.3345541358),
        (319_999, -0.3049536049),
    ];
    for (index, expected) in reference {
        let actual = output[index];
        assert!(
            (actual - expected).abs() <= 1.5e-7,
            "sample {index}: expected C# {expected:?}, got Rust {actual:?}"
        );
    }
}

#[test]
fn openutau_custom_sample_rates_match_csharp_reference_vectors() {
    // Captured from OpenUtau.Core's BiquadEQ -> SimpleCompressor -> Freeverb
    // chain at the requested sample rate. MixFxSource itself currently pins
    // 44.1 kHz; these vectors exercise the same public effect implementations
    // at rates used by other audio hosts.
    let cases = [
        (
            32_000.0,
            [
                (0, 0.161_887_556_3_f32),
                (100_000, -0.066_728_167_24),
                (319_999, -0.094_407_036_90),
            ],
        ),
        (
            48_000.0,
            [
                (0, 0.163_273_066_3_f32),
                (100_000, -0.078_952_841_46),
                (319_999, -0.102_354_034_8),
            ],
        ),
        (
            96_000.0,
            [
                (0, 0.164_294_585_6_f32),
                (100_000, -0.035_926_379_26),
                (319_999, -0.058_834_634_72),
            ],
        ),
    ];

    let input: Vec<f32> = (0..320_000)
        .map(|i| {
            let t = i as f64 * 0.000_37;
            ((t.sin() * 0.17 + (t * 0.31).cos() * 0.09) * if i % 177 == 0 { 1.4 } else { 1.0 })
                as f32
        })
        .collect();

    for (sample_rate, references) in cases {
        let mut output = input.clone();
        OpenUtauTrackFx::with_sample_rate(&test_fx(), sample_rate).process(&mut output);
        for (index, expected) in references {
            let actual = output[index];
            assert!(
                (actual - expected).abs() <= 1.5e-7,
                "{sample_rate} Hz sample {index}: expected C# {expected:?}, got Rust {actual:?}"
            );
        }
    }
}

#[test]
fn non_preset_numeric_controls_match_openutau_csharp_across_sample_rates() {
    // This deliberately combines values outside the named preset profiles:
    // high EQ cuts/boosts, a low mid frequency, a strong noninteger ratio,
    // negative makeup, near-maximum room size/pre-delay, and wet above unity.
    // C# reference values come from the same public OpenUtau effect classes.
    let cases = [
        (
            32_000.0,
            [
                (0, 0.027_780_158_45_f32),
                (100_000, -0.006_929_512_60),
                (200_000, -0.008_510_840_13),
                (319_999, -0.008_364_478_13),
            ],
        ),
        (
            48_000.0,
            [
                (0, 0.021_746_488_29_f32),
                (100_000, -0.011_130_665_43),
                (200_000, -0.026_260_508_22),
                (319_999, -0.017_168_458_55),
            ],
        ),
        (
            96_000.0,
            [
                (0, 0.016_686_988_99_f32),
                (100_000, -0.008_676_017_63),
                (200_000, -0.019_450_739_03),
                (319_999, -0.017_050_113_53),
            ],
        ),
    ];

    let mut fx = UMixFx::default();
    fx.enabled = true;
    fx.eq_low_db = 12.5;
    fx.eq_mid_freq = 97.3;
    fx.eq_mid_db = -9.75;
    fx.eq_high_db = -14.0;
    fx.comp_preset = "pop".into();
    fx.comp_threshold_db = -31.5;
    fx.comp_ratio = 7.25;
    fx.comp_makeup_db = -4.2;
    fx.reverb_preset = "hall".into();
    fx.reverb_size = 0.99;
    fx.reverb_damp = 0.05;
    fx.reverb_wet = 1.7;
    fx.reverb_pre_delay_ms = 199.5;

    let input: Vec<f32> = (0..320_000)
        .map(|i| {
            let t = i as f64 * 0.000_37;
            ((t.sin() * 0.17 + (t * 0.31).cos() * 0.09) * if i % 177 == 0 { 1.4 } else { 1.0 })
                as f32
        })
        .collect();

    for (sample_rate, references) in cases {
        let mut output = input.clone();
        OpenUtauTrackFx::with_sample_rate(&fx, sample_rate).process(&mut output);
        for (index, expected) in references {
            let actual = output[index];
            assert!(
                (actual - expected).abs() <= 5.0e-8,
                "{sample_rate} Hz sample {index}: expected C# {expected:?}, got Rust {actual:?}"
            );
        }
    }
}

#[test]
fn numeric_minimum_and_maximum_controls_match_openutau_csharp() {
    let cases = [
        (
            "minimum controls",
            UMixFx {
                enabled: true,
                eq_low_db: -24.0,
                eq_mid_freq: 20.0,
                eq_mid_db: -24.0,
                eq_high_db: -24.0,
                comp_preset: "gentle".into(),
                comp_threshold_db: -60.0,
                comp_ratio: 1.0,
                comp_makeup_db: -24.0,
                reverb_preset: "small_room".into(),
                reverb_size: 0.0,
                reverb_damp: 0.0,
                reverb_wet: 0.0,
                reverb_pre_delay_ms: 0.0,
                ..UMixFx::default()
            },
            [
                0.001_378_316_2_f32,
                -0.000_337_950_5,
                -0.000_555_886_13,
                -0.000_296_139_16,
            ],
        ),
        (
            "maximum controls",
            UMixFx {
                enabled: true,
                eq_low_db: 24.0,
                eq_mid_freq: 20_000.0,
                eq_mid_db: 24.0,
                eq_high_db: 24.0,
                comp_preset: "limit".into(),
                comp_threshold_db: 0.0,
                comp_ratio: 20.0,
                comp_makeup_db: 24.0,
                reverb_preset: "hall".into(),
                reverb_size: 1.0,
                reverb_damp: 1.0,
                reverb_wet: 2.0,
                reverb_pre_delay_ms: 200.0,
                ..UMixFx::default()
            },
            [15.678_012_f32, -7.337_366, -12.332_602_5, -11.028_531],
        ),
    ];
    let input: Vec<f32> = (0..320_000)
        .map(|i| {
            let t = i as f64 * 0.000_37;
            ((t.sin() * 0.17 + (t * 0.31).cos() * 0.09) * if i % 177 == 0 { 1.4 } else { 1.0 })
                as f32
        })
        .collect();

    for (name, fx, references) in cases {
        let mut output = input.clone();
        OpenUtauTrackFx::with_sample_rate(&fx, 44_100.0).process(&mut output);
        for (index, expected) in [0, 100_000, 200_000, 319_999].into_iter().zip(references) {
            let actual = output[index];
            let tolerance = expected.abs() * 2.0e-5 + 2.0e-6;
            assert!(
                (actual - expected).abs() <= tolerance,
                "{name} sample {index}: expected C# {expected:?}, got Rust {actual:?}"
            );
        }
    }
}

#[test]
fn numeric_control_endpoints_match_openutau_csharp_at_supported_sample_rates() {
    let cases = vec![
        (
            "minimum",
            UMixFx {
                enabled: true,
                eq_low_db: -24.0,
                eq_mid_freq: 20.0,
                eq_mid_db: -24.0,
                eq_high_db: -24.0,
                comp_preset: "gentle".into(),
                comp_threshold_db: -60.0,
                comp_ratio: 1.0,
                comp_makeup_db: -24.0,
                reverb_preset: "small_room".into(),
                reverb_size: 0.0,
                reverb_damp: 0.0,
                reverb_wet: 0.0,
                reverb_pre_delay_ms: 0.0,
                ..UMixFx::default()
            },
            vec![
                (
                    32_000.0,
                    [
                        0.001_896_223_8_f32,
                        -0.000_389_447_78,
                        -0.000_659_573_07,
                        -0.000_357_071_72,
                    ],
                ),
                (
                    48_000.0,
                    [
                        0.001_284_109_9_f32,
                        -0.000_329_957_43,
                        -0.000_539_570_76,
                        -0.000_285_929_77,
                    ],
                ),
                (
                    96_000.0,
                    [
                        0.000_833_401_6_f32,
                        -0.000_211_601_51,
                        -0.000_371_218_05,
                        -0.000_171_627_92,
                    ],
                ),
            ],
        ),
        (
            "maximum",
            UMixFx {
                enabled: true,
                eq_low_db: 24.0,
                eq_mid_freq: 20_000.0,
                eq_mid_db: 24.0,
                eq_high_db: 24.0,
                comp_preset: "limit".into(),
                comp_threshold_db: 0.0,
                comp_ratio: 20.0,
                comp_makeup_db: 24.0,
                reverb_preset: "hall".into(),
                reverb_size: 1.0,
                reverb_damp: 1.0,
                reverb_wet: 2.0,
                reverb_pre_delay_ms: 200.0,
                ..UMixFx::default()
            },
            vec![
                (32_000.0, [-7.512_205_6_f32, f32::NAN, f32::NAN, f32::NAN]),
                (
                    48_000.0,
                    [21.493_822_f32, -6.384_961, -13.340_741, -7.766_526_7],
                ),
                (
                    96_000.0,
                    [47.586_277_f32, -1.303_426_7, -9.951_792, -7.402_445],
                ),
            ],
        ),
    ];
    let input: Vec<f32> = (0..320_000)
        .map(|i| {
            let t = i as f64 * 0.000_37;
            ((t.sin() * 0.17 + (t * 0.31).cos() * 0.09) * if i % 177 == 0 { 1.4 } else { 1.0 })
                as f32
        })
        .collect();

    for (name, fx, rate_cases) in cases {
        for (sample_rate, expected) in rate_cases {
            let mut output = input.clone();
            OpenUtauTrackFx::with_sample_rate(&fx, sample_rate).process(&mut output);
            for (index, expected) in [0, 100_000, 200_000, 319_999].into_iter().zip(expected) {
                let actual = output[index];
                if expected.is_nan() {
                    assert!(
                        actual.is_nan(),
                        "{name} controls at {sample_rate} Hz, sample {index}: expected C# NaN, got Rust {actual:?}"
                    );
                    continue;
                }
                let tolerance = expected.abs() * 2.0e-5 + 2.0e-6;
                assert!(
                    (actual - expected).abs() <= tolerance,
                    "{name} controls at {sample_rate} Hz, sample {index}: expected C# {expected:?}, got Rust {actual:?}"
                );
            }
        }
    }
}

fn assert_csharp_profile(name: &str, fx: UMixFx, reference: [(usize, f32); 3]) {
    let mut output: Vec<f32> = (0..320_000)
        .map(|i| (((i * 31) % 257) as i32 - 128) as f32 / 256.0)
        .collect();
    OpenUtauTrackFx::new(&fx).process(&mut output);
    for (index, expected) in reference {
        let actual = output[index];
        assert!(
            (actual - expected).abs() <= 1.5e-7,
            "{name} sample {index}: expected C# {expected:?}, got Rust {actual:?}"
        );
    }
}

#[test]
fn all_openutau_fx_presets_match_csharp_reference_vectors() {
    let mut eq_cases = Vec::new();
    for (preset, low, freq, mid, high, samples) in [
        (
            "off",
            0.0,
            1000.0,
            0.0,
            0.0,
            [-0.2421875, 0.015625, -0.3984375],
        ),
        (
            "vocal_air",
            0.0,
            3000.0,
            1.5,
            3.0,
            [-0.2381252348, 0.09293371439, -0.5935838223],
        ),
        (
            "warm",
            2.0,
            500.0,
            0.0,
            -1.0,
            [-0.2467215955, -0.0137623474, -0.3475209773],
        ),
        (
            "demud",
            0.0,
            200.0,
            -3.0,
            0.0,
            [-0.2324593663, 0.02298877388, -0.4018251896],
        ),
        (
            "telephone",
            -6.0,
            1500.0,
            4.0,
            -6.0,
            [-0.2652954459, -0.1268252134, -0.1568301767],
        ),
    ] {
        let fx = UMixFx {
            enabled: true,
            eq_preset: preset.into(),
            eq_low_db: low,
            eq_mid_freq: freq,
            eq_mid_db: mid,
            eq_high_db: high,
            comp_ratio: 1.0,
            comp_makeup_db: 0.0,
            reverb_preset: "off".into(),
            ..UMixFx::default()
        };
        eq_cases.push((preset, fx, samples));
    }
    for (preset, fx, [at_100k, at_200k, at_end]) in eq_cases {
        assert_csharp_profile(
            &format!("EQ {preset}"),
            fx,
            [(100_000, at_100k), (200_000, at_200k), (319_999, at_end)],
        );
    }

    for (preset, threshold, ratio, makeup, samples) in [
        ("off", 0.0, 1.0, 0.0, [-0.2421875, 0.015625, -0.3984375]),
        (
            "gentle",
            -18.0,
            2.0,
            2.5,
            [-0.178680107, 0.01152937952, -0.2939375341],
        ),
        (
            "pop",
            -14.0,
            3.0,
            4.0,
            [-0.2300249636, 0.01484524831, -0.3783591986],
        ),
        ("limit", -3.0, 10.0, 0.0, [-0.2421875, 0.015625, -0.3984375]),
    ] {
        let fx = UMixFx {
            enabled: true,
            eq_low_db: 0.0,
            eq_mid_db: 0.0,
            eq_high_db: 0.0,
            comp_preset: preset.into(),
            comp_threshold_db: threshold,
            comp_ratio: ratio,
            comp_makeup_db: makeup,
            reverb_preset: "off".into(),
            ..UMixFx::default()
        };
        assert_csharp_profile(
            &format!("compressor {preset}"),
            fx,
            [
                (100_000, samples[0]),
                (200_000, samples[1]),
                (319_999, samples[2]),
            ],
        );
    }

    for (preset, size, damp, predelay, samples) in [
        ("off", 0.0, 0.0, 0.0, [-0.2421875, 0.015625, -0.3984375]),
        (
            "small_room",
            0.30,
            0.7,
            12.0,
            [-0.1770296693, 0.02114471793, -0.3745420277],
        ),
        (
            "vocal_plate",
            0.55,
            0.3,
            25.0,
            [-0.1671632528, 0.07467032969, -0.3510380387],
        ),
        (
            "hall",
            0.85,
            0.4,
            40.0,
            [-0.1587363482, -0.006778390147, -0.3768755794],
        ),
        (
            "ambient",
            0.92,
            0.2,
            60.0,
            [-0.1604446918, -0.1889888048, -0.2089081556],
        ),
    ] {
        let fx = UMixFx {
            enabled: true,
            eq_low_db: 0.0,
            eq_mid_db: 0.0,
            eq_high_db: 0.0,
            comp_ratio: 1.0,
            comp_makeup_db: 0.0,
            reverb_preset: preset.into(),
            reverb_size: size,
            reverb_damp: damp,
            reverb_pre_delay_ms: predelay,
            ..UMixFx::default()
        };
        assert_csharp_profile(
            &format!("reverb {preset}"),
            fx,
            [
                (100_000, samples[0]),
                (200_000, samples[1]),
                (319_999, samples[2]),
            ],
        );
    }
}

#[test]
fn every_openutau_fx_preset_matches_csharp_at_common_host_rates() {
    for line in include_str!("fixtures/openutau_fx_rate_vectors.csv").lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split(',').collect();
        assert_eq!(fields.len(), 6, "invalid rate-vector row: {line}");
        let family = fields[0];
        let preset = fields[1];
        let sample_rate: f64 = fields[2].parse().expect("sample rate");
        let references: [(usize, f32); 3] = [
            (100_000, fields[3].parse().expect("sample at 100000")),
            (200_000, fields[4].parse().expect("sample at 200000")),
            (319_999, fields[5].parse().expect("sample at 319999")),
        ];

        let mut fx = UMixFx {
            enabled: true,
            ..UMixFx::default()
        };
        match family {
            "eq" => {
                let (low, freq, mid, high) = match preset {
                    "off" => (0.0, 1000.0, 0.0, 0.0),
                    "vocal_air" => (0.0, 3000.0, 1.5, 3.0),
                    "warm" => (2.0, 500.0, 0.0, -1.0),
                    "demud" => (0.0, 200.0, -3.0, 0.0),
                    "telephone" => (-6.0, 1500.0, 4.0, -6.0),
                    _ => panic!("unknown EQ preset {preset}"),
                };
                fx.eq_preset = preset.into();
                fx.eq_low_db = low;
                fx.eq_mid_freq = freq;
                fx.eq_mid_db = mid;
                fx.eq_high_db = high;
                fx.comp_ratio = 1.0;
                fx.comp_makeup_db = 0.0;
                fx.reverb_preset = "off".into();
            }
            "comp" => {
                let (threshold, ratio, makeup) = match preset {
                    "off" => (0.0, 1.0, 0.0),
                    "gentle" => (-18.0, 2.0, 2.5),
                    "pop" => (-14.0, 3.0, 4.0),
                    "limit" => (-3.0, 10.0, 0.0),
                    _ => panic!("unknown compressor preset {preset}"),
                };
                fx.eq_low_db = 0.0;
                fx.eq_mid_db = 0.0;
                fx.eq_high_db = 0.0;
                fx.comp_preset = preset.into();
                fx.comp_threshold_db = threshold;
                fx.comp_ratio = ratio;
                fx.comp_makeup_db = makeup;
                fx.reverb_preset = "off".into();
            }
            "reverb" => {
                let (size, damp, predelay) = match preset {
                    "off" => (0.0, 0.0, 0.0),
                    "small_room" => (0.30, 0.7, 12.0),
                    "vocal_plate" => (0.55, 0.3, 25.0),
                    "hall" => (0.85, 0.4, 40.0),
                    "ambient" => (0.92, 0.2, 60.0),
                    _ => panic!("unknown reverb preset {preset}"),
                };
                fx.eq_low_db = 0.0;
                fx.eq_mid_db = 0.0;
                fx.eq_high_db = 0.0;
                fx.comp_ratio = 1.0;
                fx.comp_makeup_db = 0.0;
                fx.reverb_preset = preset.into();
                fx.reverb_size = size;
                fx.reverb_damp = damp;
                fx.reverb_pre_delay_ms = predelay;
            }
            _ => panic!("unknown FX family {family}"),
        }

        assert_csharp_profile_at_rate(
            &format!("{family} {preset} at {sample_rate} Hz"),
            fx,
            sample_rate,
            references,
        );
    }
}

fn assert_csharp_profile_at_rate(
    name: &str,
    fx: UMixFx,
    sample_rate: f64,
    references: [(usize, f32); 3],
) {
    let mut output: Vec<f32> = (0..320_000)
        .map(|i| ((i * 31) % 257 - 128) as f32 / 256.0)
        .collect();
    OpenUtauTrackFx::with_sample_rate(&fx, sample_rate).process(&mut output);
    for (index, expected) in references {
        let actual = output[index];
        assert!(
            (actual - expected).abs() <= 1.5e-7,
            "{name} sample {index}: expected C# {expected:?}, got Rust {actual:?}"
        );
    }
}
