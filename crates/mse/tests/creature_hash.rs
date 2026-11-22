//! Average-hash comparison of normal creatures against Magic Set Editor fixtures.
//!
//! The fixtures were exported once by the local Magic Set Editor 2.5.8, using the
//! Full Magic Pack data that ships beside that binary. The pinned pack's frame,
//! placeholder, mana, badge, and font files match those bytes. This test paints
//! with the pinned pack and does not launch Magic Set Editor.

use std::fs;
use std::path::{Path, PathBuf};

use domain::{new_creature, object_map, FMP_COMMIT, NewCreature};
use mse::{average_hash, draw_card, DrawRequest};
use serde_json::{Map, Value as Json};

struct Specimen {
    file: &'static str,
    name: &'static str,
    cost: &'static str,
    type_line: &'static str,
    text: &'static str,
    flavor: &'static str,
    power: &'static str,
    toughness: &'static str,
}

const CREATURES: &[Specimen] = &[
    Specimen {
        file: "Lantern Scout.png",
        name: "Lantern Scout",
        cost: "1W",
        type_line: "Creature - Soldier",
        text: "Vigilance",
        flavor: "",
        power: "2",
        toughness: "2",
    },
    Specimen {
        file: "Tide Heron.png",
        name: "Tide Heron",
        cost: "2U",
        type_line: "Creature - Bird",
        text: "Flying",
        flavor: "",
        power: "2",
        toughness: "2",
    },
    Specimen {
        file: "Grave Rat.png",
        name: "Grave Rat",
        cost: "B",
        type_line: "Creature - Rat",
        text: "When this creature dies, each opponent loses 1 life.",
        flavor: "",
        power: "1",
        toughness: "1",
    },
    Specimen {
        file: "Cinder Imp.png",
        name: "Cinder Imp",
        cost: "R",
        type_line: "Creature - Imp",
        text: "Haste",
        flavor: "",
        power: "1",
        toughness: "1",
    },
    Specimen {
        file: "Moss Elk.png",
        name: "Moss Elk",
        cost: "1G",
        type_line: "Creature - Elk",
        text: "Reach",
        flavor: "",
        power: "2",
        toughness: "2",
    },
    Specimen {
        file: "Void Wurm.png",
        name: "Void Wurm",
        cost: "4",
        type_line: "Creature - Eldrazi",
        text: "Annihilator 1 (Whenever this creature attacks, defending player sacrifices a permanent.)",
        flavor: "",
        power: "5",
        toughness: "5",
    },
    Specimen {
        file: "Cog Sentinel.png",
        name: "Cog Sentinel",
        cost: "3",
        type_line: "Artifact Creature - Construct",
        text: "This creature enters tapped.",
        flavor: "",
        power: "2",
        toughness: "3",
    },
    Specimen {
        file: "Prism Owl.png",
        name: "Prism Owl",
        cost: "WUB",
        type_line: "Creature - Bird",
        text: "Flying, lifelink",
        flavor: "",
        power: "2",
        toughness: "3",
    },
    Specimen {
        file: "Chapel Acolyte.png",
        name: "Chapel Acolyte",
        cost: "W",
        type_line: "Creature - Cleric",
        text: "Flying",
        flavor: "",
        power: "1",
        toughness: "1",
    },
    Specimen {
        file: "Long Sermon.png",
        name: "Long Sermon",
        cost: "2WW",
        type_line: "Creature - Cleric",
        text: "Vigilance. When this creature enters, you gain 2 life. It can block an additional creature each combat. At the beginning of your upkeep, you may pay {1}. If you do, put a +1/+1 counter on this creature.",
        flavor: "",
        power: "2",
        toughness: "4",
    },
    Specimen {
        file: "Lantern Poet.png",
        name: "Lantern Poet",
        cost: "1W",
        type_line: "Creature - Cleric",
        text: "Vigilance",
        flavor: "The lantern remembers every road.",
        power: "1",
        toughness: "2",
    },
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join("creatures")
}

fn pack_dirs() -> (PathBuf, PathBuf) {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\cjr\AppData\Roaming"))
        .join("boggled")
        .join("packs")
        .join(FMP_COMMIT);
    let data = root.join("data");
    let fonts = root.join("Magic - Fonts");
    let style = data.join("magic-m15-altered.mse-style").join("style");
    if !style.is_file() {
        panic!(
            "pinned pack not installed at {}. Run boggled pack download",
            data.display()
        );
    }
    (data, fonts)
}

fn face(item: &Specimen) -> Map<String, Json> {
    let mut fields = NewCreature::default();
    fields.name = item.name.into();
    fields.mana_cost = item.cost.into();
    fields.type_line = item.type_line.into();
    fields.text = item.text.into();
    fields.flavor_text = item.flavor.into();
    fields.power = item.power.into();
    fields.toughness = item.toughness.into();
    object_map(new_creature("FIX", fields)).unwrap()
}

fn paint(card: &Map<String, Json>, data: &Path, fonts: &Path) -> Vec<u8> {
    draw_card(DrawRequest {
        card,
        art: None,
        style_text: None,
        data_dir: Some(data),
        fonts_dir: Some(fonts),
        allow_unlisted: false,
    })
    .unwrap()
}

fn distance(left: &[u8], right: &[u8]) -> u32 {
    let left = average_hash(left).unwrap();
    let right = average_hash(right).unwrap();
    (left ^ right).count_ones()
}

#[test]
fn creature_renders_stay_near_the_mse_fixtures() {
    let fixtures = fixtures_dir();
    let (data, fonts) = pack_dirs();
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("creature-compare");
    fs::create_dir_all(&out).unwrap();

    let mut report = String::new();
    let mut worst = 0u32;
    let mut painted: Vec<(&str, Vec<u8>)> = Vec::new();
    for item in CREATURES {
        let reference = fs::read(fixtures.join(item.file)).unwrap_or_else(|_| {
            panic!("missing MSE fixture {}", fixtures.join(item.file).display())
        });
        let png = paint(&face(item), &data, &fonts);
        let bits = distance(&png, &reference);
        report.push_str(&format!("{:<20} {bits}\n", item.name));
        worst = worst.max(bits);
        let _ = fs::write(out.join(item.file), &png);
        painted.push((item.name, png));
    }

    let scout = fs::read(fixtures.join("Lantern Scout.png")).unwrap();
    let rat = fs::read(fixtures.join("Grave Rat.png")).unwrap();
    let mse_frames = distance(&scout, &rat);
    let bss_scout = painted.iter().find(|(name, _)| *name == "Lantern Scout").unwrap();
    let wrong = distance(&bss_scout.1, &rat);
    report.push_str(&format!("mse white vs black   {mse_frames}\n"));
    report.push_str(&format!("bss white vs mse black {wrong}\n"));
    report.push_str(&format!("worst fixture        {worst}\n"));
    let _ = fs::write(out.join("distances.txt"), &report);
    println!("{report}");

    assert!(
        mse_frames > worst,
        "white-vs-black MSE distance {mse_frames} is not above the fixture distances\n{report}"
    );
    assert!(
        wrong > worst,
        "a white BSS card matched the black MSE card\n{report}"
    );
    // The first measured pairs were 0 or 1 bit apart. White versus black was 29.
    // Six bits still rejects a wrong frame and leaves room for antialiasing.
    const MAX_DISTANCE: u32 = 6;
    assert!(
        worst <= MAX_DISTANCE,
        "fixture distance {worst} exceeds {MAX_DISTANCE}\n{report}"
    );
}
