// This module supports generating the app menus from the JSON manifest in the apps/ directory.

use std::{
    fs::{OpenOptions, File},
    io::{Read, Write},
    string::String,
    fmt::Write as StdWrite,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Deserialize, Serialize, Debug)]
struct AppManifest {
    context_name: String,
    menu_name: HashMap<String, HashMap<String, String>>,
    submenu: Option::<u8>,
}
#[derive(Deserialize, Serialize, Debug)]
struct Locales {
    locales: HashMap<String, HashMap<String, String>>,
}

pub(crate) fn generate_app_menus(apps: &Vec<String>) {
    let file = File::open("apps/manifest.json").expect("Failed to open the manifest file");
    let mut reader = std::io::BufReader::new(file);
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("Failed to read the file");
    let manifest: HashMap<String, AppManifest> =
        serde_json::from_str(&content).expect("Cannot parse manifest file");

    // localization file
    // inject all the localization strings into the i18n file, which in theory reduces the churn on other crates that depend
    // on the global i18n file between build variants
    let mut l = BTreeMap::<String, BTreeMap<String, String>>::new();
    for (_app, manifest) in manifest.iter() {
        for (name, translations) in &manifest.menu_name {
            let mut map = BTreeMap::<String, String>::new();
            for (language, phrase) in translations {
                map.insert(language.to_string(), phrase.to_string());
            }
            l.insert(name.to_string(), map);
        }
    }
    // output a JSON localizations file, if things have changed
    let new_i18n = serde_json::to_string(&l).unwrap();
    overwrite_if_changed(&new_i18n, "apps/i18n.json");

    // output the Rust manifests - tailored just for the apps requested
    let mut working_set = BTreeMap::<String, &AppManifest>::new();
    // derive a working_set that is just the apps we requested
    for app in apps {
        if let Some(manifest) = manifest.get(app) {
            working_set.insert(app.to_string(), &manifest);
        }
    }

    // construct the gam_tokens
    let mut gam_tokens = String::new();
    writeln!(
        gam_tokens,
        "// This file is auto-generated by xtask/main.rs generate_app_menus()"
    )
    .unwrap();
    for (app_name, manifest) in working_set.iter() {
        writeln!(
            gam_tokens,
            "pub const APP_NAME_{}: &'static str = \"{}\";",
            app_name.to_uppercase(),
            manifest.context_name,
        )
        .unwrap();
        if let Some(menu_count) = manifest.submenu {
            for i in 0..menu_count {
                writeln!(
                    gam_tokens,
                    "pub const APP_MENU_{}_{}: &'static str = \"{} Submenu {}\";",
                    i,
                    app_name.to_uppercase(),
                    manifest.context_name,
                    i,
                )
                .unwrap();
            }
        }
    }
    writeln!(
        gam_tokens,
        "\npub const EXPECTED_APP_CONTEXTS: &[&'static str] = &["
    )
    .unwrap();
    for (app_name, manifest) in working_set.iter() {
        writeln!(gam_tokens, "    APP_NAME_{},", app_name.to_uppercase(),).unwrap();
        if let Some(menu_count) = manifest.submenu {
            for i in 0..menu_count {
                writeln!(
                    gam_tokens,
                    "    APP_MENU_{}_{},",
                    i,
                    app_name.to_uppercase(),
                )
                .unwrap();
            }
        }
    }
    writeln!(gam_tokens, "];").unwrap();
    overwrite_if_changed(&gam_tokens, "services/gam/src/apps.rs");

    // construct the app menu
    let mut menu = String::new();
    writeln!(
        menu,
        "// This file is auto-generated by xtask/main.rs generate_app_menus()"
    )
    .unwrap();
    if apps.len() == 0 {
        writeln!(menu, "// NO APPS SELECTED: suppressing warning messages!").unwrap();
        writeln!(menu, "#![allow(dead_code)]").unwrap();
        writeln!(menu, "#![allow(unused_imports)]").unwrap();
        writeln!(menu, "#![allow(unused_variables)]").unwrap();
    }
    writeln!(menu, r####"use crate::StatusOpcode;
use gam::{{MenuItem, MenuPayload}};
use locales::t;
use num_traits::*;
use std::{{error::Error, fmt}};

#[derive(Debug)]
pub enum AppDispatchError {{
    IndexNotFound(usize),
}}

impl Error for AppDispatchError {{}}

impl fmt::Display for AppDispatchError {{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {{
        match self {{
            AppDispatchError::IndexNotFound(app_index) => write!(f, "Index {{}} not found", app_index),
        }}
    }}
}}

pub(crate) fn app_dispatch(gam: &gam::Gam, token: [u32; 4], index: usize) -> Result<(), AppDispatchError> {{
    match index {{"####).unwrap();
    for (index, (app_name, _manifest)) in working_set.iter().enumerate() {
        writeln!(
            menu,
            "        {} => {{
            gam.switch_to_app(gam::APP_NAME_{}, token).expect(\"couldn't raise app\");
            Ok(())
        }},",
            index,
            app_name.to_uppercase()
        )
        .unwrap();
    }
    writeln!(
        menu,
        r####"        _ => Err(AppDispatchError::IndexNotFound(index)),
    }}
}}

pub(crate) fn app_index_to_name(index: usize) -> Result<&'static str, AppDispatchError> {{
    match index {{"####
    )
    .unwrap();
    for (index, (_, _manifest)) in working_set.iter().enumerate() {
        for name in _manifest.menu_name.keys() {
            writeln!(
                menu,
                "        {} => Ok(t!(\"{}\", locales::LANG)),",
                index, name,
            )
            .unwrap();
        }
    }
    writeln!(
        menu,
        r####"        _ => Err(AppDispatchError::IndexNotFound(index)),
    }}
}}

pub(crate) fn app_menu_items(menu_items: &mut Vec::<MenuItem>, status_conn: u32) {{
"####
    )
    .unwrap();
    for (index, (_app_name, manifest)) in working_set.iter().enumerate() {
        writeln!(menu, "    menu_items.push(MenuItem {{",).unwrap();
        assert!(
            manifest.menu_name.len() == 1,
            "Improper menu name record entry"
        );
        for name in manifest.menu_name.keys() {
            writeln!(
                menu,
                "        name: xous_ipc::String::from_str(t!(\"{}\", locales::LANG)),",
                name
            )
            .unwrap();
        }
        writeln!(menu, "        action_conn: Some(status_conn),",).unwrap();
        writeln!(
            menu,
            "        action_opcode: StatusOpcode::SwitchToApp.to_u32().unwrap(),",
        )
        .unwrap();
        writeln!(
            menu,
            "        action_payload: MenuPayload::Scalar([{}, 0, 0, 0]),",
            index
        )
        .unwrap();
        writeln!(menu, "        close_on_select: true,",).unwrap();
        writeln!(menu, "    }});\n",).unwrap();
    }
    writeln!(menu, "}}").unwrap();
    overwrite_if_changed(&menu, "services/status/src/app_autogen.rs");
}

fn overwrite_if_changed(new_string: &String, old_file: &str) {
    let original = match OpenOptions::new().read(true).open(old_file) {
        Ok(mut ref_file) => {
            let mut buf = String::new();
            ref_file
                .read_to_string(&mut buf)
                .expect("UTF-8 error in previous localization file");
            buf
        }
        _ => String::new(),
    };
    if &original != new_string {
        // println!("file change in i18n.json detected:");
        // println!("Old: {}", original);
        // println!("New: {}", new_string);
        let mut new_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(old_file)
            .expect("Can't open our gam manifest for writing");
        write!(new_file, "{}", new_string).unwrap()
    }
}
