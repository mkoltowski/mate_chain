"""
MateChain — skrypt audytora paczek IPFS.
"""

import os
import sys
import json
import gzip
import hashlib
import argparse
import urllib.request
from datetime import datetime
from Crypto.Cipher import AES
from dotenv import load_dotenv

load_dotenv()

# Klucz GCM wczytywany ze zmiennej środowiskowej (plik .env), nie zapisywany w kodzie.
# Musi być identyczny z kluczem używanym przez bramę przy archiwizacji.
_aes_key_gcm = os.getenv("AES_KEY_GCM")
if not _aes_key_gcm or len(_aes_key_gcm) != 32:
    raise ValueError(
        "Brak klucza AES_KEY_GCM lub niepoprawna długość (wymagane 32 znaki = 256 bitów). "
        "Ustaw zmienną środowiskową, np. w pliku .env: AES_KEY_GCM=...."
    )
AES_KEY_GCM = _aes_key_gcm.encode("utf-8")

IPFS_LOCAL_API = "http://127.0.0.1:5001"
IPFS_GATEWAY_PUBLIC = "https://ipfs.io/ipfs"

CLR_GREEN  = "\033[32m"
CLR_RED    = "\033[31m"
CLR_CYAN   = "\033[36m"
CLR_YELLOW = "\033[33m"
CLR_BOLD   = "\033[1m"
CLR_RESET  = "\033[0m"


def naglowek(tytul):
    print(f"\n{CLR_BOLD}{CLR_CYAN}{'═' * 70}")
    print(f"  {tytul}")
    print(f"{'═' * 70}{CLR_RESET}\n")


def info(etykieta, wartosc, kolor=""):
    print(f"  {etykieta:<30} {kolor}{wartosc}{CLR_RESET}")


def sukces(tekst):
    print(f"  {CLR_GREEN}✓ {tekst}{CLR_RESET}")


def blad(tekst):
    print(f"  {CLR_RED}✗ {tekst}{CLR_RESET}")


def pobierz_z_ipfs(cid, uzyj_publicznej_bramy=False):
    if not uzyj_publicznej_bramy:
        url = f"{IPFS_LOCAL_API}/api/v0/cat?arg={cid}"
        try:
            req = urllib.request.Request(url, method="POST")
            with urllib.request.urlopen(req, timeout=10) as response:
                return response.read()
        except Exception as e:
            blad(f"Lokalny węzeł nie odpowiada: {e}")
            print(f"  {CLR_YELLOW}→ Próbuję publicznej bramy...{CLR_RESET}")
            uzyj_publicznej_bramy = True

    if uzyj_publicznej_bramy:
        url = f"{IPFS_GATEWAY_PUBLIC}/{cid}"
        try:
            with urllib.request.urlopen(url, timeout=30) as response:
                return response.read()
        except Exception as e:
            raise RuntimeError(f"Nie udało się pobrać paczki: {e}")


def weryfikuj_hash(dane, oczekiwany_hash):
    rzeczywisty_hash = hashlib.sha256(dane).hexdigest()
    info("Oczekiwany SHA-256:", oczekiwany_hash, CLR_CYAN)
    info("Obliczony SHA-256:", rzeczywisty_hash, CLR_CYAN)

    if rzeczywisty_hash.lower() == oczekiwany_hash.lower():
        sukces("Skróty się zgadzają — paczka jest autentyczna")
        return True
    else:
        blad("Skróty się NIE zgadzają — paczka mogła zostać zmodyfikowana!")
        return False


def odszyfruj_paczke(zaszyfrowane_dane):
    nonce      = zaszyfrowane_dane[:12]
    ciphertext = zaszyfrowane_dane[12:-16]
    tag        = zaszyfrowane_dane[-16:]

    info("Nonce (12 bajtów):", nonce.hex())
    info("Tag autentyczności (16 b):", tag.hex())
    info("Ciphertext:", f"{len(ciphertext)} bajtów")

    try:
        cipher = AES.new(AES_KEY_GCM, AES.MODE_GCM, nonce=nonce)
        plaintext = cipher.decrypt_and_verify(ciphertext, tag)
        sukces("Odszyfrowano poprawnie — tag GCM zweryfikowany")
        return plaintext
    except ValueError as e:
        blad(f"Błąd autentyczności GCM: {e}")
        raise


def rozpakuj_gzip(skompresowane):
    rozpakowane = gzip.decompress(skompresowane)
    wsp_kompresji = len(rozpakowane) / max(len(skompresowane), 1)
    info("Po dekompresji:", f"{len(rozpakowane)} bajtów (×{wsp_kompresji:.1f})")
    return rozpakowane


def parsuj_jsonl(jsonl_bytes):
    tekst = jsonl_bytes.decode("utf-8")
    rekordy = []
    for nr, linia in enumerate(tekst.strip().split("\n"), 1):
        try:
            rekordy.append(json.loads(linia))
        except json.JSONDecodeError as e:
            blad(f"Błąd parsowania linii {nr}: {e}")
    return rekordy


def wyswietl_rekordy(rekordy, ile=10):
    naglowek(f"Analiza zawartości paczki ({len(rekordy)} rekordów)")

    if not rekordy:
        blad("Paczka jest pusta")
        return

    temperatury = [r["wartosc"] for r in rekordy if "wartosc" in r]
    alarmy = [r for r in rekordy if r.get("signature")]

    info("Liczba odczytów:", len(rekordy))
    info("Liczba alarmów:", f"{len(alarmy)} ({100*len(alarmy)/len(rekordy):.1f}%)")
    if temperatury:
        info("Temperatura min:", f"{min(temperatury):.2f}°C")
        info("Temperatura max:", f"{max(temperatury):.2f}°C")
        info("Temperatura średnia:", f"{sum(temperatury)/len(temperatury):.2f}°C")

    print(f"\n  {CLR_BOLD}Pierwsze {min(ile, len(rekordy))} rekordów:{CLR_RESET}")
    for r in rekordy[:ile]:
        alarm_flag = f" {CLR_RED}🚨 ALARM{CLR_RESET}" if r.get("signature") else ""
        print(f"    [{r['timestamp']}] {r['container_id'][:12]} "
              f"{r.get('sensor_id','?'):<10} {r['wartosc']:6.2f}°C{alarm_flag}")

    if len(rekordy) > ile * 2:
        print(f"\n  {CLR_BOLD}... ostatnie 5 rekordów:{CLR_RESET}")
        for r in rekordy[-5:]:
            alarm_flag = f" {CLR_RED}🚨 ALARM{CLR_RESET}" if r.get("signature") else ""
            print(f"    [{r['timestamp']}] {r['container_id'][:12]} "
                  f"{r.get('sensor_id','?'):<10} {r['wartosc']:6.2f}°C{alarm_flag}")


def zapisz_raport(rekordy, cid, hash_oczekiwany, sciezka_raportu):
    raport = {
        "audyt": {
            "data": datetime.now().isoformat(),
            "narzedzie": "MateChain Audytor 1.0",
        },
        "paczka": {
            "cid": cid,
            "oczekiwany_sha256": hash_oczekiwany,
            "liczba_rekordow": len(rekordy),
        },
        "rekordy": rekordy,
    }
    with open(sciezka_raportu, "w", encoding="utf-8") as f:
        json.dump(raport, f, indent=2, ensure_ascii=False)
    sukces(f"Raport zapisany w {sciezka_raportu}")


def main():
    parser = argparse.ArgumentParser(
        description="MateChain — audytor paczek IPFS",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Przykład użycia:
  python3 audytor.py --cid QmfDJsXy1cyjRpF2WciNprERcvLE93TdFryJrP8j4KpzaA
  python3 audytor.py --cid Qm... --hash a3f8e9c2b1d4...
        """
    )
    parser.add_argument("--cid", required=True, help="Identyfikator paczki w IPFS")
    parser.add_argument("--hash", default=None, help="Oczekiwany SHA-256 (z blockchain)")
    parser.add_argument("--publiczna-brama", action="store_true",
                        help="Użyj publicznej bramki ipfs.io")
    parser.add_argument("--raport", default="raport_audytu.json",
                        help="Ścieżka do pliku z raportem")
    args = parser.parse_args()

    print(f"\n{CLR_BOLD}{'═' * 70}")
    print("           MATECHAIN — RAPORT AUDYTU PACZKI IPFS")
    print(f"{'═' * 70}{CLR_RESET}")
    info("Data audytu:", datetime.now().strftime("%Y-%m-%d %H:%M:%S"))
    info("CID paczki:", args.cid, CLR_YELLOW)

    naglowek("Krok 1/5 — pobieranie paczki z IPFS")
    zaszyfrowane = pobierz_z_ipfs(args.cid, args.publiczna_brama)
    sukces(f"Pobrano {len(zaszyfrowane)} bajtów")

    if args.hash:
        naglowek("Krok 2/5 — weryfikacja integralności (SHA-256)")
        zgodny = weryfikuj_hash(zaszyfrowane, args.hash)
        if not zgodny:
            blad("PRZERWANO AUDYT — paczka mogła zostać sfałszowana")
            sys.exit(1)
    else:
        print(f"\n  {CLR_YELLOW}⚠ Pominięto weryfikację hash (nie podano --hash){CLR_RESET}")

    naglowek("Krok 3/5 — odszyfrowanie AES-256 GCM")
    skompresowane = odszyfruj_paczke(zaszyfrowane)

    naglowek("Krok 4/5 — dekompresja gzip")
    jsonl_bytes = rozpakuj_gzip(skompresowane)

    naglowek("Krok 5/5 — parsowanie JSON Lines")
    rekordy = parsuj_jsonl(jsonl_bytes)
    sukces(f"Sparsowano {len(rekordy)} rekordów")

    wyswietl_rekordy(rekordy)

    naglowek("Zapis raportu")
    zapisz_raport(rekordy, args.cid, args.hash or "(nie podano)", args.raport)

    print(f"\n{CLR_GREEN}{CLR_BOLD}✓ AUDYT ZAKOŃCZONY POMYŚLNIE{CLR_RESET}\n")


if __name__ == "__main__":
    main()
