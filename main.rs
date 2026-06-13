use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::collections::HashMap;
use std::fs;
use serde::{Deserialize, Serialize};
use dotenvy::dotenv;

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    transaction::Transaction,
    system_program,
};

use borsh::BorshSerialize;
use sha2::{Digest, Sha256};
use aes::Aes256;
use cbc::Decryptor;
use cbc::cipher::{block_padding::Pkcs7, KeyIvInit, BlockDecryptMut};
use rusqlite::{params, Connection};

use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit};
use aes_gcm::aead::Aead;
use flate2::write::GzEncoder;
use flate2::Compression;
use rand::RngCore;

use tiny_http::{Server, Response, Header, Method};

const DB_PATH: &str = "produkcja.db";
const MODBUS_PORT: &str = "0.0.0.0:5502";
const WEBUI_PORT: &str = "0.0.0.0:8090";
const PANEL_DIR: &str = "panel";

const IPFS_API: &str = "http://127.0.0.1:5001";

const BATCH_INTERVAL_SEC: u64 = 120; //archiwista budzi się co 2 minuty
const BATCH_MIN_RECORDS: usize = 50; //ilość pacek do wyysyłki przez archiwistę

const CLR_RESET: &str = "\x1b[0m";
const CLR_RED: &str    = "\x1b[31m";
const CLR_GREEN: &str  = "\x1b[32m";
const CLR_CYAN: &str   = "\x1b[36m";
const CLR_YELLOW: &str = "\x1b[33m";
const CLR_BLUE: &str   = "\x1b[34m";

//struktury , separacja odpowiedzialności, żeby kod był czytelniejszy i łatwiejszy w utrzymaniu !

//tutaj rozpakowujemy dane z wtryskarek
#[derive(Deserialize, Debug, Clone)] //makro serde
struct SensorData {
    container_id: String,
    sensor_id: String,
    #[serde(rename = "wartosc")] //dobra praktyka w rust, zmiana wartość na value
    value: f32,
}

//pakujemy znowu, żeby wysłac do IPFS i Solany
#[derive(Serialize, Debug)] 
struct LogRecord {
    id: i64,
    container_id: String,
    sensor_id: String,
    wartosc: f32,
    signature: Option<String>, //musi być Option, jezeli nie ma awarii to None
    timestamp: String,
}

//przechowuje aktualny stan w pamięci RAM, żeby szybko odpowiadać na zapytania z WebUI i  ew. Modbus
#[derive(Clone, Serialize)]
struct StanWtryskarki {
    temp: f32,
    alarm: bool,
}

/*<Arc - nie usuwa info dopóki wszyscy ich nie pobiorą
<Mutex - wątki, wpuszcza następny dopiero jeżeli jest wolne<Hashmapa - przechowuje info w RAMIE>>>*/
type StanyMaszyn = Arc<Mutex<HashMap<String, StanWtryskarki>>>;

//mapowanie container_id na numer czujnika, żeby ładniej wyświetlać w logach i WebUI, bo container_id to długi hash
type Etykiety = Arc<Mutex<HashMap<String, u32>>>;

fn main() {
    dotenv().ok();

    let gcm_key_str = std::env::var("AES_KEY_GCM").expect("Brak klucza!");
    let bytes = gcm_key_str.as_bytes();

    if bytes.len() != 32 {
        panic!("Klucz AES_KEY_GCM musi mieć dokładnie 32 znaki!");
    }

    // Tworzymy tablicę przechowującą wartości (nie referencję)
    let mut gcm_key_val = [0u8; 32];
    gcm_key_val.copy_from_slice(bytes);

    let cbc_key_str = std::env::var("AES_KEY_CBC").expect("Brak klucza AES_KEY_CBC!");

    let mut cbc_key = [0u8; 32];
    cbc_key.copy_from_slice(cbc_key_str.as_bytes());

   

    println!("[INFO] Klucze AES załadowane z pliku .env");
    
    println!("====================================================");
    println!("MATECHAIN BRAMA — Gateway + Modbus + Archiwista + WebUI");
    println!("====================================================");

    let conn = Connection::open(DB_PATH).expect("Błąd: Nie można otworzyć bazy danych SQLite.");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS logi (
            id           INTEGER PRIMARY KEY,
            container_id TEXT,
            sensor_id    TEXT,
            wartosc      REAL,
            signature    TEXT,
            timestamp    DATETIME DEFAULT CURRENT_TIMESTAMP,
            batch_id     TEXT
        )",
        [],
    ).unwrap();
    let _ = conn.execute("ALTER TABLE logi ADD COLUMN batch_id TEXT", []);

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -10000;
         PRAGMA temp_store = MEMORY;"
    ).unwrap();
    let shared_db = Arc::new(Mutex::new(conn));
    println!("[INFO] Baza SQLite gotowa.");

    let rpc_url      = String::from("https://api.devnet.solana.com");
    let client       = Arc::new(RpcClient::new(rpc_url));
    let keypair_path = "/home/mati/.config/solana/id.json";
    let user_keypair = Arc::new(
        read_keypair_file(keypair_path).expect("Błąd: Nie znaleziono portfela Solana!")
    );
    println!("[INFO] Portfel Solana: {}", user_keypair.pubkey());

    let stany_maszyn: StanyMaszyn = Arc::new(Mutex::new(HashMap::new()));
    let etykiety: Etykiety        = Arc::new(Mutex::new(HashMap::new()));

    let stany_dla_modbus = Arc::clone(&stany_maszyn);
    thread::spawn(move || {
        uruchom_serwer_modbus(stany_dla_modbus);
    });

    let db_dla_archiwisty     = Arc::clone(&shared_db);
    let client_dla_archiwisty = Arc::clone(&client);
    let key_dla_archiwisty    = Arc::clone(&user_keypair);
    thread::spawn(move || {
        uruchom_archiwiste(db_dla_archiwisty, client_dla_archiwisty, key_dla_archiwisty, gcm_key_val);
    });

    let stany_dla_webui    = Arc::clone(&stany_maszyn);
    let db_dla_webui       = Arc::clone(&shared_db);
    let etykiety_dla_webui = Arc::clone(&etykiety);
    thread::spawn(move || {
        uruchom_serwer_webui(stany_dla_webui, db_dla_webui, etykiety_dla_webui);
    });

    let listener = TcpListener::bind("0.0.0.0:65432").expect("Błąd: Port 65432 zajęty.");
    println!("[INFO] Nasłuchuję na porcie 65432 (wtryskarki)...");
    println!("[INFO] Modbus Bridge aktywny na porcie 5502 (ScadaBR/OpenPLC)...");
    println!("[INFO] Archiwista aktywny: paczka co {}s lub {} rekordów.", BATCH_INTERVAL_SEC, BATCH_MIN_RECORDS);
    println!("[INFO] Panel WebUI aktywny: http://localhost:8090");

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let client_clone   = Arc::clone(&client);
                let key_clone      = Arc::clone(&user_keypair);
                let db_clone       = Arc::clone(&shared_db);
                let stany_clone    = Arc::clone(&stany_maszyn);
                let etykiety_clone = Arc::clone(&etykiety);
                let cbc_key_clone = cbc_key.clone();
                thread::spawn(move || {
                    obsluz_kontener(s, client_clone, key_clone, db_clone, stany_clone, etykiety_clone, cbc_key_clone);
                });
            }
            Err(e) => println!("[!] Błąd połączenia TCP: {}", e),
        }
    }
}

fn obsluz_kontener(
    mut stream: TcpStream,
    client: Arc<RpcClient>,
    user_keypair: Arc<Keypair>,
    db: Arc<Mutex<Connection>>,
    stany: StanyMaszyn,
    etykiety: Etykiety,
    cbc_key: [u8; 32]
) {
    let mut buffer = [0; 1024]; //tablicy do której wczytujemy zaszyfrowane dane z wtryskarki wysłane przez TCP

    if let Ok(size) = stream.read(&mut buffer) {
        //wektor IV a wlagorytmie AWS zawsze zajmuje 16 bajtów za wektorem leca zaszyfrowane dane
        if size < 17 { return; }

        //bierziemy wycinek 16 bajtów z bufora czyli wektor inicjalizacyjny IV, CBC
        //try.into próbuje przekonwertować wycinek na tblice o określonym rozmairze 16 bajtów
        let iv_array: [u8; 16] = match buffer[..16].try_into() { 
            Ok(arr) => arr,
            Err(_)  => return,
        };
        let ciphertext = &buffer[16..size]; 

        //deszyfracja AES-256-CBC
        let decryptor = Decryptor::<Aes256>::new((&cbc_key).into(), (&iv_array).into());
        let mut out_buffer = ciphertext.to_vec();
        //ze względów wydajnościowych deszyfrujemy na miejscu

        match decryptor.decrypt_padded_mut::<Pkcs7>(&mut out_buffer) {
            Ok(decrypted) => {
                let msg = String::from_utf8_lossy(decrypted);

                match serde_json::from_str::<SensorData>(&msg) {
                    Ok(sensor) => {
                        // Przydzielenie numeru czujnika przy pierwszym kontakcie
                        let numer_czujnika = {
                            let mut etyk = etykiety.lock().unwrap(); //blokada mutex przez wątek 
                            if let Some(&n) = etyk.get(&sensor.container_id) {
                                n //jeżeli kontener już jest to zwraca numer czujnika
                            } else {
                                let nowy = etyk.len() as u32 + 1; //jeżeli nie ma kontenera to przypisuje nowy numer 
                                etyk.insert(sensor.container_id.clone(), nowy);
                                nowy
                            }
                        }; // odblokowanie mutexa po przydzieleniu numeru

                        let is_alarm = sensor.value > 253.0 || sensor.value < 247.0;
                        let mut sig_val: Option<String> = None;
                        let etykieta = format!("Czujnik {}", numer_czujnika);

                        if is_alarm {
                            println!(
                                "[{}🚨 ALARM{}] {} | Kontener: {:<12} | {}Temp: {:>6.2}°C{}",
                                CLR_RED, CLR_RESET,
                                etykieta, sensor.container_id,
                                CLR_RED, sensor.value, CLR_RESET
                            );
                            //sklonowanie sensora ponieważ będziemy go jeszcze przesyłać dalej 
                            if let Ok(sig) = wyslij_alarm_na_solane(sensor.clone(), numer_czujnika, &client, &user_keypair) {
                                println!("[SOLANA OK] Sygnatura: {}", sig);
                                sig_val = Some(sig);
                            }
                        } else {
                            println!(
                                "[{}✅ OK   {}] {} | Kontener: {:<12} | {}Temp: {:>6.2}°C{}",
                                CLR_GREEN, CLR_RESET,
                                etykieta, sensor.container_id,
                                CLR_CYAN, sensor.value, CLR_RESET
                            );
                        }

                        zapisz_do_sql(&sensor, sig_val, &db);

                        if let Ok(mut mapa) = stany.lock() {
                            mapa.insert(sensor.container_id.clone(), StanWtryskarki {
                                temp:  sensor.value,
                                alarm: is_alarm,
                            });
                        }
                    }
                    Err(_) => println!("[!] Błąd parsowania JSON: {}", msg),
                }
            }
            Err(e) => println!("[!] Błąd deszyfracji AES: {:?}", e),
        }
    }
}
//blokujemy niepowodzenie i zapisujemy co jest, a następnie dalej
fn zapisz_do_sql(data: &SensorData, signature: Option<String>, db: &Arc<Mutex<Connection>>) {
    if let Ok(conn) = db.lock() {
        let _ = conn.execute(
            "INSERT INTO logi (container_id, sensor_id, wartosc, signature) VALUES (?1, ?2, ?3, ?4)",
            params![&data.container_id, &data.sensor_id, data.value, signature],
        );
    }
}

// ARCHIWISTA

fn uruchom_archiwiste(
    db: Arc<Mutex<Connection>>,
    client: Arc<RpcClient>,
    user_keypair: Arc<Keypair>,
    gcm_key: [u8; 32],
) {
    println!("[{}ARCHIWISTA{}] Uruchomiony.", CLR_YELLOW, CLR_RESET);

    loop {
        thread::sleep(Duration::from_secs(BATCH_INTERVAL_SEC)); //uspanie na 2 minuty

        match przetworz_paczke(&db, &client, &user_keypair, gcm_key) {
            Ok(Some((cid, count))) => {
                println!(
                    "[{}ARCHIWISTA{}] Paczka wysłana: CID={} rekordów={}",
                    CLR_YELLOW, CLR_RESET, cid, count
                );
            }
            Ok(None) => {}
            Err(e) => {
                println!("[{}ARCHIWISTA{}] Błąd: {}", CLR_RED, CLR_RESET, e);
            }
        }
    }
}

fn przetworz_paczke(
    db: &Arc<Mutex<Connection>>,
    client: &RpcClient,
    user_keypair: &Keypair,
    gcm_key: [u8; 32],
) -> Result<Option<(String, usize)>, String> {
    let rekordy = pobierz_rekordy_do_paczki(db)?; //skrót od match
    if rekordy.is_empty() { return Ok(None); }

    let liczba_rekordow = rekordy.len();
    let ids: Vec<i64> = rekordy.iter().map(|r| r.id).collect();

    //serializacja do JSON
    let mut jsonl = String::new();
    for r in &rekordy {
        jsonl.push_str(&serde_json::to_string(r).map_err(|e| e.to_string())?);
        jsonl.push('\n');
    }

    //kompresja gzip
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(jsonl.as_bytes()).map_err(|e| e.to_string())?;
    let compressed = encoder.finish().map_err(|e| e.to_string())?;

    //szyfrowanie AES-256-GCM
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&gcm_key));
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, compressed.as_ref())
        .map_err(|e| format!("AES-GCM encrypt: {}", e))?;

    //sklejanie payload
    let mut payload = nonce_bytes.to_vec();
    payload.extend_from_slice(&ciphertext);

    //obliczanie SHA-256
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let content_hash = hex::encode(hasher.finalize());

    let cid = wyslij_do_ipfs(&payload)?;

    let sig = wyslij_batch_na_solane(
        cid.clone(),
        content_hash,
        liczba_rekordow as u32,
        client,
        user_keypair,
    )?;

    oznacz_rekordy(db, &ids, &sig)?;

    Ok(Some((cid, liczba_rekordow)))
}

fn pobierz_rekordy_do_paczki(db: &Arc<Mutex<Connection>>) -> Result<Vec<LogRecord>, String> {
    let conn = db.lock().map_err(|_| "Mutex SQLite zajęty".to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, container_id, sensor_id, wartosc, signature, timestamp
                  FROM logi WHERE batch_id IS NULL ORDER BY id") //tylko rekordy niezarchiwizowane
        .map_err(|e| e.to_string())?;

    //wykonanie zapytania
    let rekordy: Vec<LogRecord> = stmt
        .query_map([], |row| {
            Ok(LogRecord {
                id:           row.get(0)?,
                container_id: row.get(1)?,
                sensor_id:    row.get(2)?,
                wartosc:      row.get(3)?,
                signature:    row.get(4)?,
                timestamp:    row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rekordy)
}
//&[i64] zamiast Vec<i64>
fn oznacz_rekordy(db: &Arc<Mutex<Connection>>, ids: &[i64], batch_sig: &str) -> Result<(), String> {
    let conn = db.lock().map_err(|_| "Mutex SQLite zajęty".to_string())?;
    for id in ids {
        conn.execute(
            "UPDATE logi SET batch_id = ?1 WHERE id = ?2",
            params![batch_sig, id],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn wyslij_do_ipfs(payload: &[u8]) -> Result<String, String> {
    let url = format!("{}/api/v0/add", IPFS_API);
    let client = reqwest::blocking::Client::new(); //klient synchroniczny
    let part = reqwest::blocking::multipart::Part::bytes(payload.to_vec()).file_name("batch.bin");
    let form = reqwest::blocking::multipart::Form::new().part("file", part);

    //wysyłka i odbiór odpowiedzi
    let response = client.post(&url).multipart(form).send()
        .map_err(|e| format!("IPFS request: {}", e))?;
    let body: serde_json::Value = response.json()
        .map_err(|e| format!("IPFS response parse: {}", e))?;

    body["Hash"].as_str().map(String::from)
        .ok_or_else(|| "IPFS: brak CID w odpowiedzi".to_string())
}

// ============================================================
// MODBUS TCP
// ============================================================

fn uruchom_serwer_modbus(stany: StanyMaszyn) {
    let listener = TcpListener::bind(MODBUS_PORT)
        .expect("Błąd: Nie można uruchomić serwera Modbus na porcie 5502.");
    println!("[MODBUS] Aktywny na {}", MODBUS_PORT);

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let stany_clone = Arc::clone(&stany);
                thread::spawn(move || { obsluz_modbus(s, stany_clone); });
            }
            Err(e) => println!("[MODBUS] Błąd połączenia: {}", e),
        }
    }
}

fn obsluz_modbus(mut stream: TcpStream, stany: StanyMaszyn) {
    let mut buf = [0u8; 256];

    //pętla odczytu
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };

        if n < 12 { continue; }

        let transaction_id = &buf[0..2];
        let unit_id        =  buf[6];
        let function_code  =  buf[7];
        let start_addr = u16::from_be_bytes([buf[8],  buf[9]])  as usize;
        let quantity   = u16::from_be_bytes([buf[10], buf[11]]) as usize;

        let snapshot: Vec<StanWtryskarki> = {
            let mapa = stany.lock().unwrap();
            let mut klucze: Vec<&String> = mapa.keys().collect();
            klucze.sort();
            klucze.iter().filter_map(|k| mapa.get(*k).cloned()).collect()
        };

        match function_code {
            0x01 => {
                let byte_count     = (quantity + 7) / 8;
                let mut coil_bytes = vec![0u8; byte_count];
                for i in 0..quantity {
                    let idx = start_addr + i;
                    if idx < snapshot.len() && snapshot[idx].alarm {
                        coil_bytes[i / 8] |= 1 << (i % 8);
                    }
                }
                wyslij_odpowiedz_modbus(&mut stream, transaction_id, unit_id, function_code, &coil_bytes);
            }
            0x04 => {
                let mut reg_bytes = Vec::with_capacity(quantity * 2);
                for i in 0..quantity {
                    let idx = start_addr + i;
                    let raw_val: u16 = if idx < snapshot.len() {
                        (snapshot[idx].temp * 10.0).clamp(0.0, 65535.0) as u16
                    } else { 0 };
                    reg_bytes.push((raw_val >> 8) as u8);
                    reg_bytes.push((raw_val & 0xFF) as u8);
                }
                wyslij_odpowiedz_modbus(&mut stream, transaction_id, unit_id, function_code, &reg_bytes);
            }
            _ => {
                let exception = [
                    transaction_id[0], transaction_id[1],
                    0x00, 0x00, 0x00, 0x03,
                    unit_id,
                    function_code | 0x80,
                    0x01,
                ];
                let _ = stream.write_all(&exception);
            }
        }
    }
}

fn wyslij_odpowiedz_modbus(
    stream: &mut TcpStream,
    transaction_id: &[u8],
    unit_id: u8,
    function_code: u8,
    data: &[u8],
) {
    let pdu_len      = 1 + 1 + 1 + data.len();
    let length_field = pdu_len as u16;

    let mut response = Vec::with_capacity(6 + pdu_len);
    response.extend_from_slice(transaction_id);
    response.extend_from_slice(&[0x00, 0x00]);
    response.push((length_field >> 8) as u8);
    response.push((length_field & 0xFF) as u8);
    response.push(unit_id);
    response.push(function_code);
    response.push(data.len() as u8);
    response.extend_from_slice(data);
    let _ = stream.write_all(&response);
}

// WEBUI

#[derive(Serialize)]
struct StanResponse {
    container_id: String,
    etykieta: String,
    temp: f32,
    alarm: bool,
}

#[derive(Serialize)]
struct AlarmRow {
    container_id: String,
    etykieta: String,
    sensor_id: String,
    wartosc: f32,
    signature: String,
    timestamp: String,
}

#[derive(Serialize)]
struct PaczkaRow {
    batch_id: String,
    records_count: i64,
    first_timestamp: String,
    last_timestamp: String,
}

#[derive(Serialize)]
struct HistoriaPunkt {
    timestamp: String,
    wartosc: f32,
}

fn uruchom_serwer_webui(stany: StanyMaszyn, db: Arc<Mutex<Connection>>, etykiety: Etykiety) {
    let server = Server::http(WEBUI_PORT).expect("Błąd: Nie można uruchomić panelu WebUI.");
    println!("[{}WEBUI{}] Aktywny na {}", CLR_BLUE, CLR_RESET, WEBUI_PORT);

    for request in server.incoming_requests() {
        if !matches!(request.method(), Method::Get) {
            let _ = request.respond(Response::from_string("405 Method Not Allowed").with_status_code(405));
            continue;
        }

        let url = request.url().to_string();
        let response = obsluz_zapytanie_webui(&url, &stany, &db, &etykiety);
        let _ = request.respond(response);
    }
}

fn obsluz_zapytanie_webui(
    url: &str,
    stany: &StanyMaszyn,
    db: &Arc<Mutex<Connection>>,
    etykiety: &Etykiety,
) -> Response<std::io::Cursor<Vec<u8>>> {
    if url == "/" || url == "/index.html" {
        return serwuj_plik("index.html", "text/html; charset=utf-8");
    }
    if url == "/styl.css" {
        return serwuj_plik("styl.css", "text/css; charset=utf-8");
    }
    if url == "/skrypt.js" {
        return serwuj_plik("skrypt.js", "application/javascript; charset=utf-8");
    }
    if url == "/api/stany" {
        return api_stany(stany, etykiety);
    }
    if url.starts_with("/api/alarmy") {
        return api_alarmy(db, etykiety);
    }
    if url.starts_with("/api/paczki") {
        return api_paczki(db);
    }
    if url.starts_with("/api/historia") {
        return api_historia(url, db, etykiety);
    }

    Response::from_string("404 Not Found").with_status_code(404)
}

fn serwuj_plik(nazwa: &str, content_type: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let sciezka = format!("{}/{}", PANEL_DIR, nazwa);
    match fs::read(&sciezka) {
        Ok(zawartosc) => Response::from_data(zawartosc)
            .with_header(Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap()),
        Err(_) => Response::from_string(format!("404: nie znaleziono pliku {}", nazwa))
            .with_status_code(404),
    }
}

fn odpowiedz_json(payload: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(payload)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap())
}

fn api_stany(stany: &StanyMaszyn, etykiety: &Etykiety) -> Response<std::io::Cursor<Vec<u8>>> {
    let mapa = stany.lock().unwrap();
    let etyk = etykiety.lock().unwrap();

    let mut lista: Vec<StanResponse> = mapa.iter().map(|(id, stan)| {
        let nr = etyk.get(id).copied().unwrap_or(0);
        StanResponse {
            container_id: id.clone(),
            etykieta: format!("Czujnik {}", nr),
            temp: stan.temp,
            alarm: stan.alarm,
        }
    }).collect();
    lista.sort_by_key(|s| etyk.get(&s.container_id).copied().unwrap_or(999));

    odpowiedz_json(serde_json::to_string(&lista).unwrap_or_else(|_| "[]".to_string()))
}

fn api_alarmy(db: &Arc<Mutex<Connection>>, etykiety: &Etykiety) -> Response<std::io::Cursor<Vec<u8>>> {
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT container_id, sensor_id, wartosc, signature, timestamp
         FROM logi WHERE signature IS NOT NULL
         ORDER BY id DESC LIMIT 20"
    ) {
        Ok(s) => s,
        Err(_) => return odpowiedz_json("[]".to_string()),
    };

    let etyk = etykiety.lock().unwrap();

    let alarmy: Vec<AlarmRow> = stmt.query_map([], |row| {
        let cid: String = row.get(0)?;
        let nr = etyk.get(&cid).copied().unwrap_or(0);
        Ok(AlarmRow {
            container_id: cid.clone(),
            etykieta:     format!("Czujnik {}", nr),
            sensor_id:    row.get(1)?,
            wartosc:      row.get(2)?,
            signature:    row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            timestamp:    row.get(4)?,
        })
    }).map(|iter| iter.filter_map(|r| r.ok()).collect()).unwrap_or_default();

    odpowiedz_json(serde_json::to_string(&alarmy).unwrap_or_else(|_| "[]".to_string()))
}

fn api_paczki(db: &Arc<Mutex<Connection>>) -> Response<std::io::Cursor<Vec<u8>>> {
    let conn = db.lock().unwrap();
    let mut stmt = match conn.prepare(
        "SELECT batch_id, COUNT(*) as cnt, MIN(timestamp) as min_ts, MAX(timestamp) as max_ts
         FROM logi WHERE batch_id IS NOT NULL
         GROUP BY batch_id ORDER BY min_ts DESC LIMIT 20"
    ) {
        Ok(s) => s,
        Err(_) => return odpowiedz_json("[]".to_string()),
    };

    let paczki: Vec<PaczkaRow> = stmt.query_map([], |row| {
        Ok(PaczkaRow {
            batch_id:        row.get(0)?,
            records_count:   row.get(1)?,
            first_timestamp: row.get(2)?,
            last_timestamp:  row.get(3)?,
        })
    }).map(|iter| iter.filter_map(|r| r.ok()).collect()).unwrap_or_default();

    odpowiedz_json(serde_json::to_string(&paczki).unwrap_or_else(|_| "[]".to_string()))
}

fn api_historia(url: &str, db: &Arc<Mutex<Connection>>, etykiety: &Etykiety) -> Response<std::io::Cursor<Vec<u8>>> {
    let id_filter = url.split('?').nth(1)
        .and_then(|q| q.split('&').find(|p| p.starts_with("id=")))
        .map(|p| p.trim_start_matches("id=").to_string());

    let conn = db.lock().unwrap();
    let query = if id_filter.is_some() {
        "SELECT container_id, wartosc, timestamp FROM logi
         WHERE container_id = ?1 AND timestamp > datetime('now', '-5 minutes')
         ORDER BY id"
    } else {
        "SELECT container_id, wartosc, timestamp FROM logi
         WHERE timestamp > datetime('now', '-5 minutes')
         ORDER BY id"
    };

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return odpowiedz_json("{}".to_string()),
    };

    let mut series: HashMap<String, Vec<HistoriaPunkt>> = HashMap::new();
    let etyk = etykiety.lock().unwrap();

    let mapper = |row: &rusqlite::Row| -> rusqlite::Result<(String, f32, String)> {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    };

    let rows = if let Some(ref id) = id_filter {
        stmt.query_map(params![id], mapper)
    } else {
        stmt.query_map([], mapper)
    };

    if let Ok(iter) = rows {
        for row in iter.filter_map(|r| r.ok()) {
            let nr = etyk.get(&row.0).copied().unwrap_or(0);
            let klucz = format!("Czujnik {}", nr);
            series.entry(klucz).or_insert_with(Vec::new).push(HistoriaPunkt {
                timestamp: row.2,
                wartosc:   row.1,
            });
        }
    }

    odpowiedz_json(serde_json::to_string(&series).unwrap_or_else(|_| "{}".to_string()))
}


// SOLANA


fn wyslij_alarm_na_solane(
    data: SensorData,
    numer_wtryskarki: u32,
    client: &RpcClient,
    user_keypair: &Keypair,
) -> Result<String, String> {
    let program_id = Pubkey::from_str("3zcbbgzgNqWLyTUJKzuzzT9t1t3VpdcuSQw9vA5qj3Zm").unwrap();
    let log_msg = format!(
        "ALARM Wtryskarka {} [czujnik temperatury] T:{:.2}C poza zakresem 247-253C",
        numer_wtryskarki, data.value
    );

    wykonaj_instrukcje_solana(
        program_id,
        "global:log_event",
        |buf| BorshSerialize::serialize(&log_msg, buf).map_err(|e| e.to_string()),
        client,
        user_keypair,
    )
}

fn wyslij_batch_na_solane(
    cid: String,
    content_hash: String,
    records_count: u32,
    client: &RpcClient,
    user_keypair: &Keypair,
) -> Result<String, String> {
    let program_id = Pubkey::from_str("3zcbbgzgNqWLyTUJKzuzzT9t1t3VpdcuSQw9vA5qj3Zm").unwrap();

    wykonaj_instrukcje_solana(
        program_id,
        "global:log_batch",
        |buf| {
            BorshSerialize::serialize(&cid, buf).map_err(|e| e.to_string())?;
            BorshSerialize::serialize(&content_hash, buf).map_err(|e| e.to_string())?;
            BorshSerialize::serialize(&records_count, buf).map_err(|e| e.to_string())?;
            Ok(())
        },
        client,
        user_keypair,
    )
}

fn wykonaj_instrukcje_solana<F>(
    program_id: Pubkey,
    metoda: &str,
    serialize_args: F,
    client: &RpcClient,
    user_keypair: &Keypair,
) -> Result<String, String>
where
    F: FnOnce(&mut Vec<u8>) -> Result<(), String>,
{
    let event_keypair = Keypair::new();

    let mut hasher = Sha256::new();
    hasher.update(metoda.as_bytes());
    let discriminator: [u8; 8] = hasher.finalize()[..8].try_into().unwrap();

    let mut instruction_data = discriminator.to_vec();
    serialize_args(&mut instruction_data)?;

    let accounts = vec![
        AccountMeta::new(event_keypair.pubkey(), true),
        AccountMeta::new(user_keypair.pubkey(), true),
        AccountMeta::new_readonly(system_program::id(), false),
    ];

    let instruction      = Instruction::new_with_bytes(program_id, &instruction_data, accounts);
    let recent_blockhash = client.get_latest_blockhash().map_err(|e| e.to_string())?;

    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&user_keypair.pubkey()),
        &[user_keypair, &event_keypair],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&tx) {
        Ok(sig) => Ok(sig.to_string()),
        Err(e)  => Err(format!("{:?}", e)),
    }
}
