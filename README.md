# rat — a GoChain terminal dashboard with a resident rat

A small [Ratatui](https://ratatui.rs) terminal UI for a GoChain (or GoChain AMM
testnet) node. One binary, two souls: a live dashboard of chain stats, blocks,
and peers, and a small ASCII rat who lives in the corner and reacts to what the
chain does.

## Run it

```bash
cargo build --release
./target/release/rat                          # polls http://127.0.0.1:8080
./target/release/rat --node http://127.0.0.1:8081   # another node
./target/release/rat --node http://127.0.0.1:8082   # the AMM testnet node (same flag)
./target/release/rat --demo                   # fake chain, no node needed
./target/release/rat --snapshot               # render one frame as text, exit
./target/release/rat --snapshot --size 80x24  # ...at a given size
```

The dashboard polls `GET /stats`, `GET /chain`, `GET /peers` (and `GET /amm`
when the node has the pool) every couple of seconds over a hand-rolled,
dependency-light HTTP client. If the node stops answering, the header flips to
OFFLINE/STALE and the rat goes to sleep. Nothing panics; the rat is patient.

## Keys

| key | does |
| --- | --- |
| `q` | quit |
| `r` | force refresh now |
| `tab` / `shift+tab` | move panel focus (border highlights) |

## The rat's states

| state | when | caption |
| --- | --- | --- |
| sleeping | node offline, or no new block for a while | "no node... nap time" / "quiet chain... small nap" |
| awake | connected, watching | "watching the wheel turn" |
| excited | a new block just landed (about 3s) | "block! the wheel turns" |
| munching | the new block carries transactions (AMM ops count double) | "a swap! snacks" |

The rat panel also keeps the NothingChain heritage alive with a
**nothing counter**: total value transferred on the chain, all time, straight
from `/stats`. On a fresh chain (and forever, on the original NothingChain) it
reads exactly `0`. There is also a small counter of blocks munched during your
visit.

## Notes

- Dependencies are deliberately minimal: `ratatui`, `crossterm`, `serde_json`.
  HTTP is a synchronous `TcpStream` GET; there is no tokio/reqwest.
- AMM operations are spotted by their `amm:` memo prefix and marked in the
  block list.
- `--snapshot` uses ratatui's `TestBackend`, which is how the layout is
  verified headlessly at 100x30 and 80x24.
