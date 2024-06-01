#![allow(unused)]

use super::structs::{Element, LatestFiles, RecordUnion};
use crate::revision_checker::Revision;
use reqwest::Client;
use std::{collections::VecDeque, io, path::PathBuf, process::exit};
use tokio::{
    fs::{create_dir_all, File},
    io::AsyncWriteExt,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Asset {
    pub filename: String,
    pub size: i64,
    pub header_size: i64,
    pub compressed_header_size: i64,
    pub crc: i64,
    pub header_crc: i64,
    pub already_fetched: bool,
}

#[derive(Debug, Clone)]
pub struct AssetFetcher {
    pub list_file_url: String,
    pub url_prefix: String,
    pub assets: VecDeque<Asset>,
    pub revision: String,
    save_path: PathBuf,
}

impl AssetFetcher {
    pub fn new(revision: Revision) -> Self {
        Self {
            assets: VecDeque::new(),
            revision: revision.clone().revision,
            url_prefix: revision.url_prefix,
            list_file_url: revision.list_file_url,
            save_path: PathBuf::from(format!("assets/{}/", revision.revision)),
        }
    }

    pub async fn load_index(&mut self) {
        // Fetches the XML version of the LatestFileList, which will be used to retrieve all assets
        let xml_url = self.list_file_url.replace("LatestFileList.bin", "LatestFileList.xml");
        if let Err(e) = self.fetch_xml(&xml_url).await {
            panic!("Error fetching LatestFileList.bin: {e}")
        }
    }

    async fn fetch_xml(&mut self, url: &str) -> io::Result<()> {
        let response = Self::request_file(url).await.unwrap();
        let xml_text = response.text().await.unwrap_or_default();

        self.parse_and_store_elements(xml_text)?;
        Ok(())
    }

    fn parse_and_store_elements(&mut self, xml_text: String) -> io::Result<()> {
        let config = quickxml_to_serde::Config::new_with_defaults();
        let json = quickxml_to_serde::xml_string_to_json(xml_text, &config).unwrap().to_string();
        let parsed: LatestFiles = serde_json::from_str(&json).unwrap();

        for (_, v) in parsed.latest_file_list {
            match v.record {
                RecordUnion::PurpleRecord(purple_record) => self.add_file_to_list(&purple_record),
                RecordUnion::RecordElementArray(records) => {
                    for r in records {
                        self.add_file_to_list(&r);
                    }
                }
            }
        }

        Ok(())
    }

    fn add_file_to_list<T: Element>(&mut self, record: &T) {
        if let Some(src_file_name) = record.get_filename() {
            let file = Asset {
                filename: src_file_name,
                size: record.get_size(),
                header_size: record.get_header_size(),
                compressed_header_size: record.get_compressed_header_size(),
                crc: record.get_crc(),
                header_crc: record.get_header_crc(),
                already_fetched: false,
            };

            self.assets.push_back(file);
        }
    }

    async fn request_file(url: &str) -> Result<reqwest::Response, reqwest::Error> {
        let client = Client::new();
        client.get(url).header("User-Agent", "KingsIsle Patcher").send().await
    }

    async fn write_to_file_chunked(path: &PathBuf, mut response: reqwest::Response) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            create_dir_all(parent).await?;
        }

        let mut file = File::create(path).await?;
        while let Some(chunk) = response.chunk().await.unwrap() {
            file.write_all(&chunk).await?;
        }

        Ok(())
    }

    pub fn fetch_asset(&self, asset: &Asset) {
        let url = format!("{}/{}", self.url_prefix, asset.filename);
        let save_path = self.save_path.clone().join(asset.filename.clone());

        tokio::spawn(async move {
            if let Ok(res) = Self::request_file(&url).await {
                if let Err(e) = Self::write_to_file_chunked(&save_path, res).await {
                    eprintln!("Error: {e}");
                    exit(e.raw_os_error().unwrap_or(1));
                }
            }
        });
    }
}
