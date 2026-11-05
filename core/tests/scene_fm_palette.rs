use cumulus_core::podcast::scene::palette::{
    HeaderDecision, MAX_RGBA_PIXELS, PaletteColor, SampleError, SceneFmBackground, background,
    hsv_to_rgb, parse_header, rgb_to_hsv, sample_rgba,
};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/scene-fm-palette-official.json")).unwrap()
}

fn color(value: &Value) -> PaletteColor {
    let channel = |key| value[key].as_i64().map(|n| i16::try_from(n).unwrap());
    PaletteColor {
        r: channel("r"),
        g: channel("g"),
        b: channel("b"),
    }
}

fn css(value: &Value) -> Option<&str> {
    if value["$undefined"] == true {
        None
    } else {
        Some(value.as_str().unwrap())
    }
}

#[test]
fn header_decision_and_all_channels_match_the_executed_official_module() {
    for case in fixture()["headers"].as_array().unwrap() {
        let parsed = parse_header(case["header"].as_str());
        if case["sample"] == true {
            assert_eq!(parsed, HeaderDecision::Sample, "{}", case["name"]);
            continue;
        }
        let HeaderDecision::Parsed(palette) = parsed else {
            panic!("truthy header must not trigger sampler: {}", case["name"]);
        };
        let expected = &case["result"];
        assert_eq!(
            palette.dominant,
            expected.get("dominantColor").map(color),
            "{}",
            case["name"]
        );
        assert_eq!(
            palette.top,
            expected.get("topColor").map(color),
            "{}",
            case["name"]
        );
        assert_eq!(
            palette.bottom,
            expected.get("bottomColor").map(color),
            "{}",
            case["name"]
        );
        assert_eq!(
            background(palette.dominant).css().as_deref(),
            css(&case["gradient"]),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn supplied_rgba_dominance_and_gradient_match_real_median_cut_and_ties() {
    for case in fixture()["quantization"].as_array().unwrap() {
        let rgba = case["rgba"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| u8::try_from(v.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            rgba.len(),
            case["width"].as_u64().unwrap() as usize
                * case["height"].as_u64().unwrap() as usize
                * 4
        );
        let actual = sample_rgba(&rgba).unwrap();
        assert_eq!(
            actual,
            Some(color(&case["dominantColor"])),
            "{}",
            case["name"]
        );
        assert_eq!(
            background(actual).css().as_deref(),
            css(&case["gradient"]),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn hsv_threshold_rounding_and_css_match_the_original_expression() {
    for case in fixture()["hsv"].as_array().unwrap() {
        let rgb = color(&case["rgb"]);
        let actual = rgb_to_hsv(rgb);
        for (field, actual) in [("h", actual.h), ("s", actual.s), ("v", actual.v)] {
            let expected = case["hsv"][field].as_f64().unwrap();
            assert!(
                (actual - expected).abs() < 1e-12,
                "{} {field}: {actual} != {expected}",
                case["name"]
            );
        }
        assert_eq!(
            hsv_to_rgb(actual).css(),
            case["roundtrip"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        assert_eq!(
            background(Some(rgb)).css().as_deref(),
            css(&case["gradient"]),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn exact_background_dto_never_serializes_raw_header_or_css() {
    assert_eq!(
        serde_json::to_value(background(None)).unwrap(),
        json!({"kind":"theme_fallback"})
    );
    assert_eq!(
        serde_json::to_value(background(Some(PaletteColor::rgb(255, 0, 0)))).unwrap(),
        json!({"kind":"gradient","first":{"r":179,"g":71,"b":71},"middle":{"r":179,"g":71,"b":89}})
    );
    let HeaderDecision::Parsed(palette) = parse_header(Some("M:12zz34")) else {
        panic!()
    };
    assert_eq!(
        serde_json::to_value(background(palette.dominant)).unwrap(),
        json!({"kind":"gradient","first":{"r":179,"g":null,"b":null},"middle":{"r":179,"g":null,"b":null}})
    );
}

#[test]
fn bounded_raw_pixels_do_not_silently_resize_or_repair_source_behavior() {
    assert_eq!(sample_rgba(&[]).unwrap(), None);
    assert_eq!(sample_rgba(&[1, 2, 3]), Err(SampleError::IncompletePixel));
    assert_eq!(
        sample_rgba(&vec![0; (MAX_RGBA_PIXELS + 1) * 4]),
        Err(SampleError::TooManyPixels)
    );
    assert_eq!(
        sample_rgba(&vec![0; MAX_RGBA_PIXELS * 4]).unwrap(),
        Some(PaletteColor::rgb(0, 0, 0))
    );
    assert_eq!(
        background(Some(PaletteColor::rgb(0, 255, 0))),
        SceneFmBackground::ThemeFallback
    );
    assert_eq!(
        background(Some(PaletteColor::rgb(0, 0, 255))),
        SceneFmBackground::ThemeFallback
    );
    assert_eq!(
        sample_rgba(&[0, 0, 0, 255, 255, 0, 0, 255]).unwrap(),
        Some(PaletteColor::rgb(0, 0, 0))
    );
}
