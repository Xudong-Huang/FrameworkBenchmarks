#[macro_use]
extern crate may;
extern crate may_minihttp;
extern crate num_cpus;
extern crate postgres;
extern crate rand;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate url;

use std::io;
use std::cmp;
use std::ops::Deref;

use url::Url;
use rand::Rng;
use postgres::{Connection, TlsMode};
use may::sync::mpmc::{self, Receiver, Sender};
use may_minihttp::{HttpServer, HttpService, Request, Response};

// thread_local!(static RNG: ThreadRng = rand::thread_rng());

#[derive(Serialize)]
struct WorldRow {
    id: i32,
    randomnumber: i32,
}

struct Techempower {
    db_rx: Receiver<Connection>,
    db_tx: Sender<Connection>,
}

struct PgConnection {
    conn: Option<Connection>,
    tx: Sender<Connection>,
}

impl Deref for PgConnection {
    type Target = Connection;

    #[inline]
    fn deref(&self) -> &Connection {
        self.conn.as_ref().unwrap()
    }
}

impl Drop for PgConnection {
    fn drop(&mut self) {
        let conn = self.conn.take().unwrap();
        self.tx.send(conn).unwrap();
    }
}

impl Techempower {
    fn get_conn(&self) -> PgConnection {
        PgConnection {
            conn: Some(self.db_rx.recv().unwrap()),
            tx: self.db_tx.clone(),
        }
    }

    fn random_world_row(&self) -> io::Result<WorldRow> {
        let conn = self.get_conn();
        let stmt = conn.prepare_cached(
            "SELECT id,randomNumber \
             FROM World WHERE id = $1",
        )?;

        let random_id = rand::thread_rng().gen_range(1, 10_000);
        let rows = &stmt.query(&[&random_id])?;
        let row = rows.get(0);

        Ok(WorldRow {
            id: row.get(0),
            randomnumber: row.get(1),
        })
    }

    fn queries(&self, req: &Request) -> io::Result<Vec<WorldRow>> {
        let url = format!("http://localhost{}", req.path());
        let url = Url::parse(&url).unwrap();
        let queries = url.query_pairs()
            .find(|pair| pair.0 == "q")
            .and_then(|(_, value)| value.parse::<u32>().ok())
            .unwrap_or(1);
        let queries = cmp::max(1, queries);
        let queries = cmp::min(500, queries) as usize;

        let conn = self.get_conn();
        let random_world = conn.prepare_cached(
            "SELECT id,randomNumber \
             FROM World WHERE id = $1",
        )?;

        let mut worlds = Vec::with_capacity(queries);
        let mut randoms = Vec::with_capacity(queries);
        let mut rng = rand::thread_rng();
        for _ in 0..queries {
            randoms.push(rng.gen_range(1, 10_000));
        }
        for i in 0..queries {
            let random_id = randoms[i];
            let rows = &random_world.query(&[&random_id])?;
            let row = rows.get(0);

            worlds.push(WorldRow {
                id: row.get(0),
                randomnumber: row.get(1),
            });
        }
        Ok(worlds)
    }
}

impl HttpService for Techempower {
    fn call(&self, req: Request) -> io::Result<Response> {
        let mut resp = Response::new();

        // Bare-bones router
        match req.path() {
            "/json" => {
                let msg = json!({"message": "Hello, World!"});
                resp.header("Content-Type", "application/json");
                *resp.body_mut() = serde_json::to_vec(&msg).unwrap();
            }
            "/plaintext" => {
                resp.header("Content-Type", "text/plain")
                    .body("Hello, World!");
            }
            "/db" => {
                let msg = self.random_world_row().expect("failed to get random world");
                resp.header("Content-Type", "application/json");
                *resp.body_mut() = serde_json::to_vec(&msg).unwrap();
            }

            p => {
                if p.starts_with("/queries") {
                    let msg = self.queries(&req).expect("failed to get queries");
                    resp.header("Content-Type", "application/json");
                    *resp.body_mut() = serde_json::to_vec(&msg).unwrap();
                } else {
                    resp.status_code(404, "Not Found");
                }
            }
        }

        Ok(resp)
    }
}

fn main() {
    may::config()
        .set_io_workers(num_cpus::get())
        .set_workers(num_cpus::get());

    let dbhost = match option_env!("DBHOST") {
        Some(it) => it,
        _ => "localhost",
    };

    let db_url = format!(
        "postgres://benchmarkdbuser:benchmarkdbpass@{}/hello_world",
        dbhost
    );

    // create the connection pool
    let (db_tx, db_rx) = mpmc::channel();
    may::coroutine::scope(|s| {
        for _ in 0..(num_cpus::get() * 4) {
            go!(s, || {
                let conn = Connection::connect(db_url.as_str(), TlsMode::None).unwrap();
                db_tx.send(conn).unwrap();
            });
        }
    });

    let techempower = Techempower { db_rx, db_tx };
    let server = HttpServer(techempower).start("0.0.0.0:8080").unwrap();
    server.wait();
}
