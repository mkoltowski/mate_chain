# MateChain

Kryptograficzny system klasy Przemysł 4.0 do zabezpieczania logów produkcyjnych.

### O projekcie
System rozwiązuje problem braku niezaprzeczalności danych telemetrycznych w fabrykach i środowiskach przemysłowych. Dane pobierane z maszyn produkcyjnych są natychmiast szyfrowane i zabezpieczane na kilku poziomach. Moim głównym założeniem było zapisywanie w sieci blockchain wyłącznie dowodów kryptograficznych, co zapewnia ogromną optymalizację kosztów oraz gwarantuje pełną odporność na sabotaż.

### Dlaczego taka architektura
Tradycyjne zapisywanie wszystkich logów produkcyjnych na blockchainie jest nieefektywne i bardzo drogie. Zastosowałem więc hybrydowe podejście. Pełne dane są szyfrowane lokalnie i cyklicznie archiwizowane w zdecentralizowanej sieci IPFS. Następnie na blockchain Solana trafia wyłącznie sygnatura pliku oraz hash weryfikujący integralność paczki. W przypadku nagłych awarii lub drastycznych odchyleń temperatury krytyczny alarm wysyłany jest natychmiast jako osobna transakcja na smart contract.

### Wykorzystane technologie
Zbudowałem środowisko składające się z wydajnej bramy sieciowej połączonej z aplikacją interfejsu wizualnego. Główny rdzeń systemu napisałem w języku Rust, zapewniając bezpieczne zarządzanie pamięcią w wielu wątkach. Inteligentne kontrakty uruchamiane w sieci Solana powstały przy użyciu frameworka Anchor. Skrypty symulujące czujniki wtryskarek oraz narzędzie audytora napisałem w języku Python. Całe środowisko zostało w pełni skonteneryzowane przy pomocy narzędzi Docker i Podman.

### Praca Inżynierska
Projekt ten powstał jako praktyczna realizacja mojej pracy dyplomowej pod tytułem "Zastosowanie mechanizmu łańcucha bloków do śledzenia newralgicznych zdarzeń produkcyjnych". Cały proces badawczy, analiza decyzji architektonicznych oraz obszerna dokumentacja znajdują się w pliku PDF umieszczonym w głównym katalogu tego repozytorium. Zachęcam do lektury.

### Uruchomienie lokalne
Aby postawić cały system na swoim komputerze, wystarczy użyć narzędzia Docker Compose. Odpowiednie polecenie automatycznie podniesie lokalny węzeł IPFS, bramę w języku Rust oraz wszystkie kontenery z symulatorami maszyn przemysłowych. Panel sterowania będzie od razu gotowy do podglądu pod adresem localhost na porcie 8090.
