use std::collections::HashMap;
use std::default::Default;
use std::env;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use headless_chrome::browser::tab::EventListener;
use headless_chrome::protocol::page::ScreenshotFormat;
use headless_chrome::protocol::Event;
use headless_chrome::protocol::Event::Lifecycle;
use headless_chrome::{Browser, LaunchOptionsBuilder};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const ARTICLE_ROOT_DIV_CLASS: &str = ".sc-1vjtieq-0";
const WAIT_AFTER_LAST_IDLE_MS: u128 = 1000;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let username =
        env::var("CREATOR_ID").expect("Set your creator ID to environment variable CREATOR_ID");
    let to_id = env::var("TO_ID").unwrap_or("0".to_owned()).parse::<u32>()?;
    let from_id = env::var("FROM_ID")
        .unwrap_or("10000000000".to_owned())
        .parse::<u32>()?;
    let start_url = format!(
        "https://api.fanbox.cc/post.listCreator?creatorId={}&limit=10",
        username
    );
    let client = reqwest::Client::new();
    let mut url = start_url.to_owned();
    let launch_options = LaunchOptionsBuilder::default()
        .window_size(Some((1024, 1024 * 1024)))
        .build()
        .unwrap();
    let browser = Browser::new(launch_options)?;
    let _ = browser.wait_for_initial_tab()?;

    loop {
        let ret = process_page(&username, to_id, from_id, &url, &client, &browser).await?;
        match ret {
            Some(next) => url = next,
            None => return Ok(()),
        }
    }
}

async fn process_page(
    username: &String,
    to_id: u32,
    from_id: u32,
    url: &String,
    client: &Client,
    browser: &Browser,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let origin = format!("https://{}.fanbox.cc", username);
    let res = client.get(url).header("Origin", origin).send().await?;

    println!("Url: {} Status: {}", url, res.status());

    let data = res.text().await?;
    let root: Root = serde_json::from_str(&data).unwrap();

    if root.body.items.len() == 0 {
        println!("No more items. Finish");
        return Ok(None);
    }

    for item in &root.body.items {
        let id = item.id.parse::<u32>()?;
        if id <= to_id {
            println!("Reach end ID {}. Finish", item.id);
            return Ok(None);
        }
        if id > from_id {
            println!("Skipping ID {}", item.id);
            continue;
        }
        if let Some(body) = &item.body {
            let dir = format!("out/{}", item.id);
            fs::create_dir_all(&dir)?;

            let title = format!("{}-{}", item.id, item.title);
            let mut body_html: String = "".to_owned();
            if let Some(txt) = &body.text {
                body_html = txt.to_owned();
            }

            let fanbox_url = format!("https://{}.fanbox.cc/posts/{}", username, item.id);

            if let Some(blocks) = &body.blocks {
                for block in blocks {
                    if let Some(txt) = &block.text {
                        body_html.push_str("<p>");
                        body_html.push_str(txt);
                        body_html.push_str("</p>");
                    }
                    if let Some(id) = &block.image_id {
                        let ext = &body.image_map[id].extension;
                        let fname = format!("./{}.{}", id, ext);
                        body_html.push_str("<p><img src=\"");
                        body_html.push_str(&fname);
                        body_html.push_str("\" /></p>");
                    }
                }
            }

            let html = format!(
                r#"<html><head><meta http-equiv="Content-Type" content="text/html; charset=UTF-8"></head><body>
      <p>
        id: {}<br />
        title: {}<br />
        published: {}<br />
        updated: {}<br />
      </p>
      {}
    </body></html>"#,
                item.id, item.title, item.published_datetime, item.updated_datetime, body_html
            );

            let fname = format!("{}/{}.html", dir, title);
            let mut dest = File::create(&fname)?;
            dest.write_all(html.as_bytes())?;

            let pdf_name = format!("{}/{}", dir, title);
            let file_size = save_article(browser, fanbox_url, pdf_name)?;

            println!("Output {} (pdf size: {})", title, file_size);
        } else {
            println!("Skipping empty body. ID {}", item.id);
        }
    }

    Ok(root.body.next_url)
}

struct EventHandler {
    start_time: Instant,
    last_network_idle_ms: AtomicU64,
}

impl EventHandler {
    fn is_idle(&self) -> bool {
        let now = Instant::now();
        return match self.last_network_idle_ms.load(Ordering::Relaxed) {
            0 => false,
            v => {
                now.duration_since(self.start_time).as_millis() - v as u128
                    > WAIT_AFTER_LAST_IDLE_MS
            }
        };
    }
}

impl EventListener<Event> for EventHandler {
    fn on_event(&self, event: &Event) -> () {
        match event {
            Lifecycle(l) => {
                if l.params.name == "networkIdle" {
                    self.last_network_idle_ms.fetch_max(
                        Instant::now().duration_since(self.start_time).as_millis() as u64,
                        Ordering::SeqCst,
                    );
                }
            }
            _ => {}
        }
    }
}

fn save_article(
    browser: &Browser,
    url: String,
    filename: String,
) -> Result<usize, Box<dyn std::error::Error>> {
    let tab = browser.new_tab()?;
    let handler = Arc::new(EventHandler {
        start_time: Instant::now(),
        last_network_idle_ms: AtomicU64::new(0),
    });
    tab.add_event_listener(handler.clone())?;
    tab.navigate_to(&url)?;
    tab.wait_until_navigated()?;

    while !handler.is_idle() {
        thread::sleep(Duration::from_millis(100));
    }

    let viewport = tab
        .wait_for_element(ARTICLE_ROOT_DIV_CLASS)?
        .get_box_model()?
        .margin_viewport();
    let jpg = tab.capture_screenshot(ScreenshotFormat::JPEG(None), Some(viewport), true)?;
    let mut jpg_file = File::create(&format!("{}.jpg", filename))?;
    if jpg.is_empty() {
        println!("Could not generate jpg");
    } else {
        jpg_file.write_all(&jpg)?;
    }
    let pdf = tab.print_to_pdf(None)?;
    let mut pdf_file = File::create(&format!("{}.pdf", filename))?;
    pdf_file.write_all(&pdf)?;
    Ok(pdf.len())
}

#[derive(Serialize, Deserialize, Debug)]
struct Root {
    body: Body,
}

#[derive(Serialize, Deserialize, Debug)]
struct Body {
    items: Vec<Item>,
    #[serde(rename = "nextUrl")]
    next_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Item {
    body: Option<ItemBody>,
    #[serde(rename = "coverImageUrl", default)]
    cover_imageurl: Option<String>,
    id: String,
    #[serde(rename = "publishedDatetime")]
    published_datetime: String,
    title: String,
    #[serde(rename = "updatedDatetime")]
    updated_datetime: String, // YYYY-MM-DD'T'HH:MM:SSZZZZZ
}

#[derive(Serialize, Deserialize, Debug)]
struct ItemBody {
    text: Option<String>,       // text
    blocks: Option<Vec<Block>>, // blog
    #[serde(rename = "imageMap", default)]
    image_map: HashMap<String, ImageMapValue>,
    #[serde(rename = "fileMap", default)]
    file_map: HashMap<String, String>,
    #[serde(rename = "embedMap", default)]
    embed_map: HashMap<String, EmbedMapValue>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Block {
    #[serde(rename = "type")]
    block_type: String, // p or image
    #[serde(default)]
    text: Option<String>,
    #[serde(rename = "imageId", default)]
    image_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ImageMapValue {
    id: String,
    extension: String,
    width: i32,
    height: i32,
    #[serde(rename = "originalUrl")]
    original_url: String,
    #[serde(rename = "thumbnailUrl")]
    thumbnail_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct EmbedMapValue {
    id: String,
    #[serde(rename = "serviceProvider")]
    service_provider: String,
    #[serde(rename = "contentId")]
    contenet_id: String,
}
