//! App state: node polling, JSON wrangling, demo data, and the rat's brain.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::http;
use crate::Args;

pub const FETCH_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Clone, Default)]
pub struct Stats {
    pub chain_id: String,
    pub height: i64,
    pub blocks: i64,
    pub pending: i64,
    pub supply: String,
    pub reward: String,
    pub next_halving: String,
    pub tip: String,
    pub transferred: String,
    pub unit: String,
    pub valid: Option<bool>,
}

#[derive(Clone)]
pub struct BlockInfo {
    pub index: i64,
    pub hash: String,
    pub txs: usize,
    pub has_amm: bool,
    pub miner: String,
}

#[derive(Clone)]
pub struct PeerInfo {
    pub url: String,
    pub remote_height: i64,
    pub remote_tip: String,
    pub rtt_ms: i64,
    pub error: String,
    pub adopted: i64,
}

#[derive(Clone)]
pub struct AmmInfo {
    pub go_reserve: String,
    pub usd_reserve: String,
    pub fee_bps: i64,
}

#[derive(Clone, Default)]
pub struct ChainData {
    pub stats: Stats,
    pub blocks: Vec<BlockInfo>, // latest first
    pub peers: Vec<PeerInfo>,
    pub amm: Option<AmmInfo>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Blocks,
    Peers,
    Rat,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Offline,
    Sleep,
    Awake,
    Excited,
    Munch,
}

pub struct Rat {
    pub excited_until: Instant,
    pub munch_until: Instant,
    pub last_event: Instant,
    pub last_tip: String,
    pub blocks_munched: u64,
}

impl Rat {
    fn new(now: Instant) -> Rat {
        Rat {
            excited_until: now,
            munch_until: now,
            last_event: now,
            last_tip: String::new(),
            blocks_munched: 0,
        }
    }

    /// Watch a fresh chain snapshot and react to what changed.
    fn observe(&mut self, data: &ChainData, now: Instant) {
        let tip = data.stats.tip.clone();
        if tip.is_empty() || tip == self.last_tip {
            return;
        }
        let first = self.last_tip.is_empty();
        self.last_tip = tip;
        self.last_event = now;
        if first {
            return; // the first snapshot is just the rat moving in
        }
        self.blocks_munched += 1;
        self.excited_until = now + Duration::from_secs(3);
        if let Some(latest) = data.blocks.first() {
            if latest.txs > 0 {
                let snack_for = if latest.has_amm { 4 } else { 3 };
                self.munch_until = now + Duration::from_secs(snack_for);
            }
        }
    }

    pub fn mood(&self, online: bool, now: Instant) -> Mood {
        if !online {
            return Mood::Offline;
        }
        if now < self.excited_until {
            return Mood::Excited;
        }
        if now < self.munch_until {
            return Mood::Munch;
        }
        if now.duration_since(self.last_event) > Duration::from_secs(45) {
            return Mood::Sleep;
        }
        Mood::Awake
    }

    pub fn caption(&self, mood: Mood, demo: bool) -> &'static str {
        match mood {
            Mood::Offline => "no node... nap time",
            Mood::Sleep => "quiet chain... small nap",
            Mood::Excited => "block! the wheel turns",
            Mood::Munch => "a swap! snacks",
            Mood::Awake => {
                if demo {
                    "watching the fake wheel turn"
                } else {
                    "watching the wheel turn"
                }
            }
        }
    }
}

pub struct App {
    pub node: String,
    pub demo: bool,
    pub data: Option<ChainData>,
    pub online: bool,
    pub last_ok: Option<Instant>,
    pub last_attempt: Option<Instant>,
    pub focus: Focus,
    pub tick: u64,
    pub rat: Rat,
    demo_gen: DemoGen,
}

impl App {
    pub fn new(args: &Args) -> App {
        let now = Instant::now();
        App {
            node: args.node.clone(),
            demo: args.demo,
            data: None,
            online: false,
            last_ok: None,
            last_attempt: None,
            focus: Focus::Blocks,
            tick: 0,
            rat: Rat::new(now),
            demo_gen: DemoGen::new(now),
        }
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn poll(&mut self) {
        let now = Instant::now();
        self.last_attempt = Some(now);
        if self.demo {
            let data = self.demo_gen.step(now);
            self.online = true;
            self.last_ok = Some(now);
            self.rat.observe(&data, now);
            self.data = Some(data);
            return;
        }
        match fetch_all(&self.node) {
            Some(data) => {
                self.online = true;
                self.last_ok = Some(now);
                self.rat.observe(&data, now);
                self.data = Some(data);
            }
            None => {
                self.online = false;
            }
        }
    }

    /// Seconds since the last successful poll, if any ever succeeded.
    pub fn stale_secs(&self) -> Option<u64> {
        self.last_ok.map(|t| t.elapsed().as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;

    fn demo_app() -> App {
        App::new(&Args {
            node: String::new(),
            demo: true,
            snapshot: true,
            size: (100, 30),
        })
    }

    #[test]
    fn rat_gets_excited_when_a_new_block_lands() {
        let mut app = demo_app();
        app.poll(); // first snapshot: the rat just moves in, no excitement
        assert_eq!(app.rat.blocks_munched, 0);
        app.poll(); // demo mines a fresh block with a new tip
        assert!(
            app.rat.blocks_munched >= 1,
            "expected the rat to notice the new block"
        );
        assert_eq!(app.rat.mood(true, Instant::now()), Mood::Excited);
    }

    #[test]
    fn rat_sleeps_when_offline() {
        let app = demo_app();
        assert_eq!(app.rat.mood(false, Instant::now()), Mood::Offline);
    }
}

// ---------- live fetching ----------

fn fetch_all(base: &str) -> Option<ChainData> {
    let stats_body = http::get(&format!("{base}/stats"), FETCH_TIMEOUT).ok()?;
    let stats_json: Value = serde_json::from_str(&stats_body).ok()?;
    let stats = parse_stats(&stats_json);

    let mut blocks = Vec::new();
    if let Ok(body) = http::get(&format!("{base}/chain"), FETCH_TIMEOUT) {
        if let Ok(v) = serde_json::from_str::<Value>(&body) {
            blocks = parse_chain(&v);
        }
    }
    let mut peers = Vec::new();
    if let Ok(body) = http::get(&format!("{base}/peers"), FETCH_TIMEOUT) {
        if let Ok(v) = serde_json::from_str::<Value>(&body) {
            peers = parse_peers(&v);
        }
    }
    let amm = http::get(&format!("{base}/amm"), FETCH_TIMEOUT)
        .ok()
        .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        .and_then(|v| parse_amm(&v));

    Some(ChainData {
        stats,
        blocks,
        peers,
        amm,
    })
}

fn jstr(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn ji(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(0)
}

fn parse_stats(v: &Value) -> Stats {
    Stats {
        chain_id: jstr(v, "chain_id"),
        height: ji(v, "height"),
        blocks: ji(v, "blocks"),
        pending: ji(v, "pending_txs"),
        supply: jstr(v, "total_supply"),
        reward: jstr(v, "block_reward"),
        next_halving: jstr(v, "next_halving"),
        tip: jstr(v, "tip_hash"),
        transferred: jstr(v, "total_transferred"),
        unit: {
            let u = jstr(v, "unit");
            if u.is_empty() {
                "GO".to_string()
            } else {
                u
            }
        },
        valid: v.get("valid").and_then(|x| x.as_bool()),
    }
}

fn parse_chain(v: &Value) -> Vec<BlockInfo> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<BlockInfo> = arr
        .iter()
        .map(|b| {
            let txs = b.get("txs").and_then(|t| t.as_array());
            let has_amm = txs.map_or(false, |ts| {
                ts.iter().any(|tx| {
                    tx.get("memo")
                        .and_then(|m| m.as_str())
                        .map_or(false, |m| m.starts_with("amm:"))
                })
            });
            BlockInfo {
                index: ji(b, "index"),
                hash: jstr(b, "hash"),
                txs: txs.map_or(0, |t| t.len()),
                has_amm,
                miner: jstr(b, "miner"),
            }
        })
        .collect();
    out.reverse(); // node returns oldest first; we show latest first
    out
}

fn parse_peers(v: &Value) -> Vec<PeerInfo> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .map(|p| PeerInfo {
            url: jstr(p, "url"),
            remote_height: ji(p, "remote_height"),
            remote_tip: jstr(p, "remote_tip"),
            rtt_ms: ji(p, "round_trip_ms"),
            error: jstr(p, "last_error"),
            adopted: ji(p, "adopted_chains"),
        })
        .collect()
}

fn parse_amm(v: &Value) -> Option<AmmInfo> {
    let go = jstr(v, "go_reserve");
    let usd = jstr(v, "testusd_reserve");
    if go.is_empty() && usd.is_empty() {
        return None;
    }
    Some(AmmInfo {
        go_reserve: go,
        usd_reserve: usd,
        fee_bps: ji(v, "fee_bps"),
    })
}

// ---------- demo data ----------

/// A tiny deterministic-ish fake chain so the UI can be shown off offline.
struct DemoGen {
    height: i64,
    blocks: Vec<BlockInfo>,
    tip: String,
    counter: u64,
}

impl DemoGen {
    fn new(_now: Instant) -> DemoGen {
        let mut g = DemoGen {
            height: 0,
            blocks: Vec::new(),
            tip: String::new(),
            counter: 0,
        };
        for _ in 0..8 {
            g.mine(false);
        }
        g
    }

    fn fake_hash(&mut self) -> String {
        // xorshift64: stable, dependency-free "hashes"
        let mut x = 0x9E3779B97F4A7C15u64 ^ (self.counter.wrapping_add(1) * 0x2545F4914F6CDD1D);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.counter = self.counter.wrapping_add(1);
        format!("{x:016x}{x:016x}")
    }

    fn mine(&mut self, with_tx_chance: bool) {
        self.height += 1;
        self.tip = self.fake_hash();
        let roll = self.counter % 10;
        let (txs, has_amm) = if with_tx_chance && roll < 4 {
            (1 + (roll % 3) as usize, roll == 3)
        } else {
            (0, false)
        };
        let miner = format!("go1demo{:04x}miner{:04x}", self.counter % 0xffff, self.height);
        self.blocks.insert(
            0,
            BlockInfo {
                index: self.height - 1,
                hash: self.tip.clone(),
                txs,
                has_amm,
                miner,
            },
        );
        self.blocks.truncate(40);
    }

    fn step(&mut self, _now: Instant) -> ChainData {
        // Each poll (~2s) usually mines a block; sometimes the wheel rests.
        if self.counter % 5 != 4 {
            self.mine(true);
        } else {
            self.counter = self.counter.wrapping_add(1);
        }
        let supply = self.height * 10;
        let stats = Stats {
            chain_id: "gochain-minute-leap-v2 (demo)".to_string(),
            height: self.height - 1,
            blocks: self.height,
            pending: (self.counter % 3) as i64,
            supply: supply.to_string(),
            reward: "10".to_string(),
            next_halving: "2028-02-29".to_string(),
            tip: self.tip.clone(),
            transferred: "0".to_string(),
            unit: "GO".to_string(),
            valid: Some(true),
        };
        let peers = vec![
            PeerInfo {
                url: "http://127.0.0.1:8081".to_string(),
                remote_height: self.height - 1,
                remote_tip: self.tip.clone(),
                rtt_ms: 12,
                error: String::new(),
                adopted: 1,
            },
            PeerInfo {
                url: "http://127.0.0.1:8082".to_string(),
                remote_height: self.height - 2,
                remote_tip: self.fake_hash(),
                rtt_ms: 31,
                error: String::new(),
                adopted: 0,
            },
        ];
        let amm = Some(AmmInfo {
            go_reserve: "1250".to_string(),
            usd_reserve: "124980".to_string(),
            fee_bps: 30,
        });
        ChainData {
            stats,
            blocks: self.blocks.clone(),
            peers,
            amm,
        }
    }
}
