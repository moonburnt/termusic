/*
 * model.rs
 * Copyright (C) 2019 gmg137 <gmg137@live.com>
 * Distributed under terms of the GPLv3 license.
 */

pub mod encrypt;
mod model;

use super::SongTag;
use anyhow::{anyhow, bail, Result};
use bytes::Buf;
use encrypt::Crypto;
use lazy_static::lazy_static;
use lofty::picture::Picture;
use model::{to_lyric, to_song_info, to_song_url, Method, Parse, SongUrl};
use regex::Regex;
use reqwest::{Client, ClientBuilder};
use std::{collections::HashMap, time::Duration};

lazy_static! {
    static ref _CSRF: Regex = Regex::new(r"_csrf=(?P<csrf>[^(;|$)]+)").unwrap();
}

const BASE_URL_NETEASE: &str = "https://music.163.com";

const USER_AGENT_LIST: [&str; 14] = [
    "Mozilla/5.0 (iPhone; CPU iPhone OS 9_1 like Mac OS X) AppleWebKit/601.1.46 (KHTML, like Gecko) Version/9.0 Mobile/13B143 Safari/601.1",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 9_1 like Mac OS X) AppleWebKit/601.1.46 (KHTML, like Gecko) Version/9.0 Mobile/13B143 Safari/601.1",
    "Mozilla/5.0 (Linux; Android 5.0; SM-G900P Build/LRX21T) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/59.0.3071.115 Mobile Safari/537.36",
    "Mozilla/5.0 (Linux; Android 6.0; Nexus 5 Build/MRA58N) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/59.0.3071.115 Mobile Safari/537.36",
    "Mozilla/5.0 (Linux; Android 5.1.1; Nexus 6 Build/LYZ28E) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/59.0.3071.115 Mobile Safari/537.36",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 10_3_2 like Mac OS X) AppleWebKit/603.2.4 (KHTML, like Gecko) Mobile/14F89;GameHelper",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 10_0 like Mac OS X) AppleWebKit/602.1.38 (KHTML, like Gecko) Version/10.0 Mobile/14A300 Safari/602.1",
    "Mozilla/5.0 (iPad; CPU OS 10_0 like Mac OS X) AppleWebKit/602.1.38 (KHTML, like Gecko) Version/10.0 Mobile/14A300 Safari/602.1",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.12; rv:46.0) Gecko/20100101 Firefox/46.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_12_5) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/59.0.3071.115 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_12_5) AppleWebKit/603.2.4 (KHTML, like Gecko) Version/10.1.1 Safari/603.2.4",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:46.0) Gecko/20100101 Firefox/46.0",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/51.0.2704.103 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/42.0.2311.135 Safari/537.36 Edge/13.1058",
];

pub struct Api {
    client: Client,
    csrf: String,
}

#[derive(Clone, Copy)]
enum CryptoApi {
    Weapi,
}

// types: 单曲(1)，歌手(100)，专辑(10)，歌单(1000)，用户(1002) *(type)*
// types: single (1), singer (100), album (10), playlist (1000), user (1002) *(type)*
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum SearchRequestType {
    Single = 1,
    Singer = 100,
    Album = 10,
    Playlist = 1000,
    User = 1002,
}

impl Api {
    pub fn new() -> Self {
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("build client error");

        Self {
            client,
            csrf: String::new(),
        }
    }

    // 发送请求
    // method: 请求方法
    // path: 请求路径
    // params: 请求参数
    // cryptoapi: 请求加密方式
    // ua: 要使用的 USER_AGENT_LIST
    async fn request(
        &mut self,
        method: Method,
        path: &str,
        params: HashMap<&str, &str>,
        cryptoapi: CryptoApi,
        ua: &str,
    ) -> Result<String> {
        let url = format!("{BASE_URL_NETEASE}{path}");
        match method {
            Method::Post => {
                let user_agent = match cryptoapi {
                    CryptoApi::Weapi => choose_user_agent(ua).to_string(),
                };
                let body = match cryptoapi {
                    CryptoApi::Weapi => {
                        let mut params = params;
                        params.insert("csrf_token", &self.csrf[..]);
                        let text = serde_json::to_string(&params)?;

                        Crypto::weapi(&text)?
                    }
                };

                let response = self
                    .client
                    .post(&url)
                    .header("Cookie", "os=pc; appver=2.7.1.198277")
                    .header("Accept", "*/*")
                    .header("Accept-Encoding", "gzip,deflate")
                    .header("Accept-Language", "en-US,en;q=0.5")
                    .header("Connection", "keep-alive")
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .header("Host", "music.163.com")
                    .header("Referer", "https://music.163.com")
                    .header("User-Agent", user_agent)
                    .body(body)
                    .send()
                    .await?;

                if self.csrf.is_empty() {
                    let value = response.headers().get("set-cookie");
                    if let Some(v) = value {
                        let v = v.to_str()?;
                        if v.contains("__csrf") {
                            let csrf_token =
                                _CSRF
                                    .captures(v)
                                    .map_or("", |caps| match caps.name("csrf") {
                                        Some(c) => c.as_str(),
                                        None => "",
                                    });
                            csrf_token.clone_into(&mut self.csrf);
                        }
                    }
                }
                Ok(response.text().await?)
            }
            Method::Get => Ok(self.client.get(&url).send().await?.text().await?),
        }
    }

    // 搜索
    // keywords: 关键词
    // offset: 起始点
    // limit: 数量
    pub async fn search(
        &mut self,
        keywords: &str,
        types: SearchRequestType,
        offset: u16,
        limit: u16,
    ) -> Result<Vec<SongTag>> {
        let path = "/weapi/search/get";
        let mut params = HashMap::new();
        let types_str = (types as usize).to_string();
        let offset = &offset.to_string();
        let limit = &limit.to_string();
        params.insert("s", keywords);
        params.insert("type", types_str.as_str());
        params.insert("offset", offset);
        params.insert("limit", limit);
        let result = self
            .request(Method::Post, path, params, CryptoApi::Weapi, "")
            .await?;

        match types {
            SearchRequestType::Single => {
                let songtag_vec =
                    to_song_info(&result, Parse::Search).ok_or_else(|| anyhow!("Search Error"))?;
                Ok(songtag_vec)
            }
            _ => bail!("None Error"),
        }
    }

    // 查询歌词
    // music_id: 歌曲id
    pub async fn song_lyric(&mut self, music_id: &str) -> Result<String> {
        let csrf_token = self.csrf.clone();
        let path = "/weapi/song/lyric";
        let mut params = HashMap::new();
        params.insert("id", music_id);
        params.insert("lv", "-1");
        params.insert("tv", "-1");
        params.insert("csrf_token", &csrf_token);
        let result = self
            .request(Method::Post, path, params, CryptoApi::Weapi, "")
            .await?;
        to_lyric(&result).ok_or_else(|| anyhow!("Search Error"))
    }

    // 歌曲 URL
    // ids: 歌曲列表
    pub async fn songs_url(&mut self, ids: &[u64]) -> Result<Vec<SongUrl>> {
        let csrf_token = self.csrf.clone();
        let path = "/weapi/song/enhance/player/url/v1";
        let mut params = HashMap::new();
        let ids = serde_json::to_string(ids)?;

        params.insert("ids", ids.as_str());
        params.insert("level", "standard");
        params.insert("encodeType", "aac");
        params.insert("csrf_token", &csrf_token);
        let result = self
            .request(Method::Post, path, params, CryptoApi::Weapi, "")
            .await?;
        to_song_url(&result).ok_or_else(|| anyhow!("Search Error"))
    }

    pub async fn song_url(&mut self, id: &str) -> Result<String> {
        let song_id_u64 = id.parse::<u64>()?;

        let result = self.songs_url(&[song_id_u64]).await?;
        if result.is_empty() {
            bail!("None Error");
        }

        let r = result.first().ok_or_else(|| anyhow!("None Error"))?;
        Ok(r.url.to_string())
    }

    // download picture
    pub async fn pic(&mut self, pic_id: &str) -> Result<Picture> {
        let id_encrypted = Crypto::encrypt_id(pic_id);
        let url = format!("https://p3.music.126.net/{id_encrypted}/{pic_id}.jpg?param=300y300");

        let result = self.client.get(url).send().await?;

        let mut reader = result.bytes().await?.reader();
        let picture = Picture::from_reader(&mut reader)?;
        Ok(picture)
    }
}

fn choose_user_agent(ua: &str) -> &str {
    let index = if ua == "mobile" {
        rand::random::<usize>() % 7
    } else if ua == "pc" {
        rand::random::<usize>() % 5 + 8
    } else if ua.is_empty() {
        rand::random::<usize>() % USER_AGENT_LIST.len()
    } else {
        return ua;
    };
    USER_AGENT_LIST[index]
}
