use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io::prelude::*;

use headless_chrome::{Browser};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let username =
        env::var("CREATOR_ID").expect("Set your creator ID to environment variable CREATOR_ID");
    let to_id = env::var("TO_ID").unwrap_or("".to_owned());
    let start_url = format!(
        "https://api.fanbox.cc/post.listCreator?creatorId={}&limit=10",
        username
    );
    let client = reqwest::Client::new();
    let mut url = start_url.to_owned();
    let browser = Browser::default()?;
    let _ = browser.wait_for_initial_tab()?;

    loop {
        let ret = process_page(&username, &to_id, &url, &client, &browser).await?;
        match ret {
            Some(next) => url = next,
            None => return Ok(()),
        }
    }
}

async fn process_page(
    username: &String,
    to_id: &String,
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
        if item.id == *to_id {
            println!("Reach end ID {}. Finish", item.id);
            return Ok(None);
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

            let fname = format!("{}/index.html", dir);
            let mut dest = File::create(&fname)?;
            dest.write_all(html.as_bytes())?;

            let pdf_name = format!("{}/{}.pdf", dir, title);
            let file_size = save_article(browser, fanbox_url, pdf_name)?;

            println!("Output {} (pdf size: {})", title, file_size);
        } else {
            println!("Skipping empty body. ID {}", item.id);
        }
    }

    Ok(root.body.next_url)
}


fn save_article(browser: &Browser, url: String, filename: String) -> Result<usize, Box<dyn std::error::Error>> {
    let tab = browser.new_tab()?;
    tab.navigate_to(&url)?;
    tab.wait_until_navigated()?;
    let data = tab.print_to_pdf( None )?;
    let mut dest = File::create(&filename)?;
    dest.write_all(&data)?;
    Ok(data.len())
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
