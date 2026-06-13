import socket
import json
import time
import random
import os
from Crypto.Cipher import AES
from Crypto.Util.Padding import pad
from dotenv import load_dotenv

load_dotenv()

cbc_key_str = os.getenv("AES_KEY_CBC")
if not cbc_key_str or len(cbc_key_str) != 32:
    raise ValueError("Klucz CBC musi mieć dokładnie 32 znaki!")

SECRET_KEY = cbc_key_str.encode('utf-8') 

RUST_HOST = os.getenv('RUST_HOST', '172.17.0.1') #Pobiera adres bramy z kontenera z pliku compose.yml,
# jeśli nie ma zmiennej to bierze domyślny adres 172.17.0.1 dla podmana 
RUST_PORT = int(os.getenv('RUST_PORT', 65432)) #tutaj pobiera port z compose.yml

CONTAINER_ID = socket.gethostname() #host systemu

SENSOR_PHYS_ID = os.getenv('SENSOR_ID', 'sensor_X') #id czujnika, jeśli nie sensor_X

def encrypt(message):
    cipher = AES.new(SECRET_KEY, AES.MODE_CBC) #nowy obiekt, tryb CBC
    ct_bytes = cipher.encrypt(pad(message.encode('utf-8'), AES.block_size)) #szyfrowanie wiadomości plus padding 
    return cipher.iv + ct_bytes 

def run():
    print(f"Symulator wtryskarki | Kontener: {CONTAINER_ID} | Czujnik: {SENSOR_PHYS_ID}") #start kontenera, do logów

    current_temp = 250.0 #temperatura startowa polipropylenu
    trend = 0.0 #start bez trendu

    while True:
        try:
            # Szum termopary 
            noise = random.uniform(-0.05, 0.05)

            # Awarie, za kazdym razem funkcja losuję nową liczbę  
            if random.random() < 0.003:
                trend = random.uniform(3.0, 5.0)    
            elif random.random() < 0.003:
                trend = random.uniform(-5.0, -3.0) 
            elif random.random() < 0.01:
                trend = random.uniform(-0.3, 0.3)

            if current_temp > 251: trend -= 0.3
            if current_temp < 249: trend += 0.3

            # Szybszy powrót do normy
            trend *= 0.75

            current_temp += trend + noise

            payload = json.dumps({
                "container_id": str(CONTAINER_ID),
                "sensor_id": str(SENSOR_PHYS_ID),
                "wartosc": round(current_temp, 2)
            })

            encrypted_data = encrypt(payload)
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s: #AF_INET to IPv4, SOCK_STREAM to TCP 
                s.settimeout(3) #jęsli brama nie odpowie w ciągu 3 sekund, zabezpieczenie przed zawieszeniem
                s.connect((RUST_HOST, RUST_PORT)) #łączenie z bramą, adres i port z compose.yml
                s.sendall(encrypted_data) #wysyłanie zaszyfrowanych danych do bramy

            time.sleep(random.uniform(3, 6))

        except Exception as e:
            print(f"[-] Błąd komunikacji brzegowej: {e}")
            time.sleep(random.uniform(3, 5)) 

if __name__ == "__main__":
    run()