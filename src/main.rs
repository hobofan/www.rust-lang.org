#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate lazy_static;
extern crate rand;
extern crate reqwest;
extern crate serde_json;
#[macro_use]
extern crate rocket;
extern crate rust_team_data;
extern crate sass_rs;
extern crate siphasher;
extern crate toml;

extern crate rocket_contrib;
extern crate serde;
#[macro_use]
extern crate serde_derive;

extern crate fluent_bundle;
extern crate regex;

extern crate handlebars;

mod cache;
mod caching;
mod category;
mod headers;
mod i18n;
mod production;
mod redirect;
mod rust_version;
mod teams;

use production::User;

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::fs::File;
use std::hash::Hasher;
use std::io::prelude::*;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};

use rand::seq::SliceRandom;

use rocket::{
    http::{RawStr, Status},
    request::{FromParam, Request},
    response::{NamedFile, Redirect},
};

use rocket_contrib::templates::Template;

use sass_rs::{compile_file, Options};

use category::Category;

use caching::{Cached, Caching};
use i18n::{I18NHelper, SupportedLocale, TeamHelper};
use rocket::http::hyper::header::CacheDirective;

lazy_static! {
    static ref ASSETS: AssetFiles = {
        let app_css_file = compile_sass("app");
        let fonts_css_file = compile_sass("fonts");
        let vendor_css_file = concat_vendor_css(vec!["skeleton", "tachyons"]);
        let app_js_file = concat_app_js(vec!["tools-install"]);

        AssetFiles {
            css: CSSFiles {
                app: app_css_file,
                fonts: fonts_css_file,
                vendor: vendor_css_file,
            },
            js: JSFiles { app: app_js_file },
        }
    };
}

#[derive(Serialize)]
struct Context<T: ::serde::Serialize> {
    page: String,
    title: String,
    parent: String,
    is_landing: bool,
    data: T,
    lang: String,
    baseurl: String,
    pontoon_enabled: bool,
    assets: AssetFiles,
}
#[derive(Clone, Serialize)]
struct CSSFiles {
    app: String,
    fonts: String,
    vendor: String,
}
#[derive(Clone, Serialize)]
struct JSFiles {
    app: String,
}
#[derive(Clone, Serialize)]
struct AssetFiles {
    css: CSSFiles,
    js: JSFiles,
}

static LAYOUT: &str = "components/layout";
static ENGLISH: &str = "en-US";

fn pontoon_enabled() -> bool {
    env::var("RUST_WWW_PONTOON").is_ok()
}

fn baseurl(lang: &str) -> String {
    if lang == "en-US" {
        String::new()
    } else {
        format!("/{}", lang)
    }
}

fn get_title(page_name: &str) -> String {
    let mut v: Vec<char> = page_name.replace("-", " ").chars().collect();
    v[0] = v[0].to_uppercase().nth(0).unwrap();
    let page_name = String::from_iter(v);
    format!("{} - Rust programming language", page_name).to_string()
}

#[get("/components/<_file..>", rank = 1)]
fn components(_file: PathBuf) -> Template {
    not_found_locale(ENGLISH.into())
}

#[get("/<locale>/components/<_file..>", rank = 11)]
fn components_locale(locale: SupportedLocale, _file: PathBuf) -> Template {
    not_found_locale(locale.0)
}

#[get("/logos/<file..>", rank = 1)]
fn logos(file: PathBuf) -> Option<Cached<NamedFile>> {
    NamedFile::open(Path::new("static/logos").join(file))
        .ok()
        .map(|file| file.cached(vec![CacheDirective::MaxAge(3600)]))
}

#[get("/static/<file..>", rank = 1)]
fn files(file: PathBuf) -> Option<Cached<NamedFile>> {
    NamedFile::open(Path::new("static/").join(file))
        .ok()
        .map(|file| file.cached(vec![CacheDirective::MaxAge(3600)]))
}

#[get("/")]
fn index() -> Template {
    render_index(ENGLISH.into())
}

#[get("/favicon.ico", rank = 0)]
fn favicon() -> Option<Cached<NamedFile>> {
    NamedFile::open("static/images/favicon.ico")
        .ok()
        .map(|file| file.cached(vec![CacheDirective::MaxAge(3600)]))
}

#[get("/<locale>", rank = 3)]
fn index_locale(locale: SupportedLocale) -> Template {
    render_index(locale.0)
}

#[get("/<category>")]
fn category(category: Category) -> Template {
    render_category(category, ENGLISH.into())
}

#[get("/<locale>/<category>", rank = 11)]
fn category_locale(category: Category, locale: SupportedLocale) -> Template {
    render_category(category, locale.0)
}

#[get("/governance")]
fn governance() -> Result<Template, Status> {
    render_governance(ENGLISH.into())
}

#[get("/governance/<section>/<team>", rank = 2)]
fn team(section: String, team: String) -> Result<Template, Result<Redirect, Status>> {
    render_team(section, team, ENGLISH.into())
}

#[get("/<locale>/governance", rank = 10)]
fn governance_locale(locale: SupportedLocale) -> Result<Template, Status> {
    render_governance(locale.0)
}

#[get("/<locale>/governance/<section>/<team>", rank = 12)]
fn team_locale(
    section: String,
    team: String,
    locale: SupportedLocale,
) -> Result<Template, Result<Redirect, Status>> {
    render_team(section, team, locale.0)
}

#[get("/production/users")]
fn production() -> Template {
    render_production(ENGLISH.into())
}

#[get("/<locale>/production/users", rank = 10)]
fn production_locale(locale: SupportedLocale) -> Template {
    render_production(locale.0)
}

#[get("/<category>/<subject>", rank = 4)]
fn subject(category: Category, subject: String) -> Template {
    render_subject(category, subject, ENGLISH.into())
}

#[get("/<locale>/<category>/<subject>", rank = 14)]
fn subject_locale(category: Category, subject: String, locale: SupportedLocale) -> Template {
    render_subject(category, subject, locale.0)
}

fn load_users_data() -> Vec<Vec<User>> {
    let mut rng = rand::thread_rng();
    let mut users = production::get_info().expect("couldn't get production users data");
    users.shuffle(&mut rng);
    users.chunks(3).map(|s| s.to_owned()).collect()
}

#[get("/<dest>", rank = 19)]
fn redirect(dest: redirect::Destination) -> Redirect {
    Redirect::permanent(dest.uri)
}

#[get("/pdfs/<dest>")]
fn redirect_pdfs(dest: redirect::Destination) -> Redirect {
    Redirect::permanent("/static/pdfs/".to_owned() + dest.uri)
}
#[get("/en-US", rank = 1)]
fn redirect_bare_en_us() -> Redirect {
    Redirect::permanent("/")
}

#[get("/<_locale>", rank = 20)]
fn redirect_bare_locale(_locale: redirect::Locale) -> Redirect {
    Redirect::temporary("/")
}

#[get("/en-US/<dest>", rank = 1)]
fn redirect_en_us(dest: redirect::Destination) -> Redirect {
    Redirect::permanent(dest.uri)
}

#[get("/<_locale>/<dest>", rank = 20)]
fn redirect_locale(_locale: redirect::Locale, dest: redirect::Destination) -> Redirect {
    // Temporary until locale support is restored.
    Redirect::temporary(dest.uri)
}

#[catch(404)]
fn not_found(req: &Request) -> Template {
    let lang = if let Some(next) = req.uri().segments().next() {
        if let Ok(lang) = SupportedLocale::from_param(RawStr::from_str(next)) {
            lang.0
        } else {
            ENGLISH.into()
        }
    } else {
        ENGLISH.into()
    };

    not_found_locale(lang)
}

fn not_found_locale(lang: String) -> Template {
    let page = "404";
    let title = format!("{} - Rust programming language", page).to_string();
    let context = Context {
        page: "404".to_string(),
        title,
        parent: LAYOUT.to_string(),
        is_landing: false,
        data: (),
        lang,
        baseurl: String::new(),
        pontoon_enabled: pontoon_enabled(),
        assets: ASSETS.clone(),
    };
    Template::render(page, &context)
}

#[catch(500)]
fn catch_error() -> Template {
    not_found_locale(ENGLISH.into())
}

fn hash_css(css: &str) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(css.as_bytes());
    hasher.finish().to_string()
}

fn compile_sass(filename: &str) -> String {
    let scss_file = format!("./src/styles/{}.scss", filename);

    let css = compile_file(&scss_file, Options::default())
        .expect(&format!("couldn't compile sass: {}", &scss_file));

    let css_sha = format!("{}_{}", filename, hash_css(&css));
    let css_file = format!("./static/styles/{}.css", css_sha);

    let mut file =
        File::create(&css_file).expect(&format!("couldn't make css file: {}", &css_file));
    file.write_all(&css.into_bytes())
        .expect(&format!("couldn't write css file: {}", &css_file));

    String::from(&css_file[1..])
}

fn concat_vendor_css(files: Vec<&str>) -> String {
    let mut concatted = String::new();
    for filestem in files {
        let vendor_path = format!("./static/styles/{}.css", filestem);
        let contents = fs::read_to_string(vendor_path).expect("couldn't read vendor css");
        concatted.push_str(&contents);
    }

    let css_sha = format!("vendor_{}", hash_css(&concatted));
    let css_path = format!("./static/styles/{}.css", &css_sha);

    fs::write(&css_path, &concatted).expect("couldn't write vendor css");

    String::from(&css_path[1..])
}

fn concat_app_js(files: Vec<&str>) -> String {
    let mut concatted = String::new();
    for filestem in files {
        let vendor_path = format!("./static/scripts/{}.js", filestem);
        let contents = fs::read_to_string(vendor_path).expect("couldn't read app js");
        concatted.push_str(&contents);
    }

    let js_sha = format!("app_{}", hash_css(&concatted));
    let js_path = format!("./static/scripts/{}.js", &js_sha);

    fs::write(&js_path, &concatted).expect("couldn't write app js");

    String::from(&js_path[1..])
}

fn render_index(lang: String) -> Template {
    #[derive(Serialize)]
    struct IndexData {
        rust_version: String,
        rust_release_post: String,
    }

    let page = "index".to_string();
    let title = "Rust programming language".to_string();

    let context = Context {
        page: page.clone(),
        title,
        parent: LAYOUT.to_string(),
        is_landing: true,
        data: IndexData {
            rust_version: rust_version::rust_version().unwrap_or(String::new()),
            rust_release_post: rust_version::rust_release_post().map_or(String::new(), |v| {
                format!("https://blog.rust-lang.org/{}", v)
            }),
        },
        baseurl: baseurl(&lang),
        lang,
        pontoon_enabled: pontoon_enabled(),
        assets: ASSETS.clone(),
    };
    Template::render(page, &context)
}

fn render_category(category: Category, lang: String) -> Template {
    let page = category.index();
    let title = get_title(&category.name());
    let context = Context {
        page: category.name().to_string(),
        title,
        parent: LAYOUT.to_string(),
        is_landing: false,
        data: (),
        baseurl: baseurl(&lang),
        lang,
        pontoon_enabled: pontoon_enabled(),
        assets: ASSETS.clone(),
    };
    Template::render(page, &context)
}

fn render_production(lang: String) -> Template {
    let page = "production/users".to_string();
    let title = "Users - Rust programming language".to_string();
    let context = Context {
        page: page.clone(),
        title,
        parent: LAYOUT.to_string(),
        is_landing: false,
        data: load_users_data(),
        baseurl: baseurl(&lang),
        lang,
        pontoon_enabled: pontoon_enabled(),
        assets: ASSETS.clone(),
    };
    Template::render(page, &context)
}

fn render_governance(lang: String) -> Result<Template, Status> {
    match teams::index_data() {
        Ok(data) => {
            let page = "governance/index".to_string();
            let title = "Governance - Rust programming language".to_string();
            let context = Context {
                page: page.clone(),
                title,
                parent: LAYOUT.to_string(),
                is_landing: false,
                data,
                baseurl: baseurl(&lang),
                lang,
                pontoon_enabled: pontoon_enabled(),
                assets: ASSETS.clone(),
            };
            Ok(Template::render(page, &context))
        }
        Err(err) => {
            eprintln!("error while loading the governance page: {}", err);
            Err(Status::InternalServerError)
        }
    }
}

fn render_team(
    section: String,
    team: String,
    lang: String,
) -> Result<Template, Result<Redirect, Status>> {
    match teams::page_data(&section, &team) {
        Ok(data) => {
            let page = "governance/group".to_string();
            let title = get_title(&data.team.website_data.as_ref().unwrap().name);
            let context = Context {
                page: page.clone(),
                title,
                parent: LAYOUT.to_string(),
                is_landing: false,
                data,
                baseurl: baseurl(&lang),
                lang,
                pontoon_enabled: pontoon_enabled(),
                assets: ASSETS.clone(),
            };
            Ok(Template::render(page, &context))
        }
        Err(err) => {
            if err.is::<teams::TeamNotFound>() {
                match (section.as_str(), team.as_str()) {
                    // Old teams URLs
                    ("teams", "language-and-compiler") => {
                        Err(Ok(Redirect::temporary("/governance")))
                    }
                    _ => Err(Err(Status::NotFound)),
                }
            } else {
                eprintln!("error while loading the team page: {}", err);
                Err(Err(Status::InternalServerError))
            }
        }
    }
}

fn render_subject(category: Category, subject: String, lang: String) -> Template {
    let page = format!("{}/{}", category.name(), subject.as_str()).to_string();
    let title = get_title(&subject);
    let context = Context {
        page: subject,
        title,
        parent: LAYOUT.to_string(),
        is_landing: false,
        data: (),
        baseurl: baseurl(&lang),
        lang,
        pontoon_enabled: pontoon_enabled(),
        assets: ASSETS.clone(),
    };
    Template::render(page, &context)
}

fn main() {
    let templating = Template::custom(|engine| {
        engine
            .handlebars
            .register_helper("text", Box::new(I18NHelper::new()));
        engine
            .handlebars
            .register_helper("team-text", Box::new(TeamHelper::new()));
    });

    rocket::ignite()
        .attach(templating)
        .attach(headers::InjectHeaders)
        .mount(
            "/",
            routes![
                index,
                category,
                governance,
                team,
                production,
                subject,
                files,
                favicon,
                logos,
                components,
                index_locale,
                category_locale,
                governance_locale,
                team_locale,
                production_locale,
                subject_locale,
                components_locale,
                redirect,
                redirect_pdfs,
                redirect_bare_en_us,
                redirect_bare_locale,
                redirect_en_us,
                redirect_locale
            ],
        )
        .register(catchers![not_found, catch_error])
        .launch();
}
