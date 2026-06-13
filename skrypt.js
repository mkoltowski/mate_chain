const KOLORY_LINII = ['#00ff9f', '#38b6ff', '#ffcc00', '#ff66cc', '#ff6633', '#66ffcc'];

function skrot(tekst, znaki = 8) {
    if (!tekst || tekst.length < znaki * 2) return tekst;
    return tekst.slice(0, znaki) + '…' + tekst.slice(-znaki);
}

function sformatujCzas(iso) {
    const d = new Date(iso.replace(' ', 'T') + 'Z');
    if (isNaN(d.getTime())) return iso;
    const dzien = String(d.getDate()).padStart(2, '0');
    const mies = String(d.getMonth() + 1).padStart(2, '0');
    return `${dzien}.${mies} ${d.toLocaleTimeString('pl-PL')}`;
}

function tikajZegar() {
    document.getElementById('zegar').textContent = new Date().toLocaleTimeString('pl-PL');
}

async function pobierzStany() {
    try {
        const r = await fetch('/api/stany');
        const dane = await r.json();
        renderujMaszyny(dane);
        document.getElementById('licznik-maszyn').textContent = dane.length;
    } catch (e) {
        console.error('Błąd /api/stany', e);
    }
}

function renderujMaszyny(maszyny) {
    const kontener = document.getElementById('lista-maszyn');
    if (!maszyny.length) {
        kontener.innerHTML = '<div class="info-pusta">Oczekiwanie na dane...</div>';
        return;
    }

    kontener.innerHTML = maszyny.map(m => `
        <div class="kafelek-maszyny">
            <div class="led ${m.alarm ? 'alarm' : 'ok'}"></div>
            <div class="maszyna-info">
                <span class="maszyna-id">${m.etykieta}</span>
                <span class="maszyna-temp ${m.alarm ? 'alarm' : ''}">${m.temp.toFixed(1)}°C</span>
            </div>
        </div>
    `).join('');
}

async function pobierzAlarmy() {
    try {
        const r = await fetch('/api/alarmy');
        const dane = await r.json();
        renderujAlarmy(dane);
        document.getElementById('licznik-alarmow').textContent = dane.length;
    } catch (e) {
        console.error('Błąd /api/alarmy', e);
    }
}

function renderujAlarmy(alarmy) {
    const kontener = document.getElementById('lista-alarmow');
    if (!alarmy.length) {
        kontener.innerHTML = '<div class="info-pusta">Brak alarmów</div>';
        return;
    }

    kontener.innerHTML = alarmy.map(a => `
        <div class="zdarzenie alarm">
            <div class="zdarzenie-naglowek">
                <span>${a.etykieta}</span>
                <span style="color: var(--akcent-alarm)">${a.wartosc.toFixed(2)}°C</span>
            </div>
            <a class="zdarzenie-sygnatura"
               href="https://explorer.solana.com/tx/${a.signature}?cluster=devnet"
               target="_blank">${skrot(a.signature, 12)}</a>
            <span class="zdarzenie-czas">${sformatujCzas(a.timestamp)}</span>
        </div>
    `).join('');
}

async function pobierzPaczki() {
    try {
        const r = await fetch('/api/paczki');
        const dane = await r.json();
        renderujPaczki(dane);
        document.getElementById('licznik-paczek').textContent = dane.length;
    } catch (e) {
        console.error('Błąd /api/paczki', e);
    }
}

function renderujPaczki(paczki) {
    const kontener = document.getElementById('lista-paczek');
    if (!paczki.length) {
        kontener.innerHTML = '<div class="info-pusta">Brak paczek</div>';
        return;
    }

    kontener.innerHTML = paczki.map(p => `
        <div class="zdarzenie paczka">
            <div class="zdarzenie-naglowek">
                <span>${p.records_count} rekordów</span>
                <span style="color: var(--akcent-zolty)">paczka</span>
            </div>
            <a class="zdarzenie-cid"
               href="https://explorer.solana.com/tx/${p.batch_id}?cluster=devnet"
               target="_blank">sygnatura: ${skrot(p.batch_id, 12)}</a>
            <span class="zdarzenie-czas">${sformatujCzas(p.first_timestamp)} → ${sformatujCzas(p.last_timestamp)}</span>
        </div>
    `).join('');
}

let wykres = null;

function utworzWykresProsty() {
    const ctx = document.getElementById('wykres-temp').getContext('2d');
    wykres = new Chart(ctx, {
        type: 'line',
        data: { labels: [], datasets: [] },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            animation: false,
            scales: {
                x: {
                    grid: { color: '#2a3142' },
                    ticks: { color: '#7a8599', maxTicksLimit: 8 }
                },
                y: {
                    min: 240, max: 260,
                    grid: { color: '#2a3142' },
                    ticks: { color: '#7a8599' },
                    title: { display: true, text: '°C', color: '#7a8599' }
                }
            },
            plugins: {
                legend: { labels: { color: '#d8dee9' } }
            }
        }
    });
}

async function odswiezWykres() {
    try {
        const r = await fetch('/api/historia');
        const serie = await r.json();

        const wszystkieEtykiety = new Set();
        Object.values(serie).forEach(punkty => {
            punkty.forEach(p => wszystkieEtykiety.add(p.timestamp));
        });
        const etykiety = Array.from(wszystkieEtykiety).sort();
        const etykiety_skrocone = etykiety.map(t => t.split(' ')[1] || t);

        const seriePosortowane = Object.entries(serie).sort((a, b) => a[0].localeCompare(b[0]));

        const datasets = seriePosortowane.map(([nazwa, punkty], idx) => {
            const mapa = Object.fromEntries(punkty.map(p => [p.timestamp, p.wartosc]));
            return {
                label: nazwa,
                data: etykiety.map(t => mapa[t] ?? null),
                borderColor: KOLORY_LINII[idx % KOLORY_LINII.length],
                backgroundColor: KOLORY_LINII[idx % KOLORY_LINII.length] + '22',
                borderWidth: 2,
                pointRadius: 0,
                tension: 0.3,
                spanGaps: true
            };
        });

        wykres.data.labels = etykiety_skrocone;
        wykres.data.datasets = datasets;
        wykres.update();
    } catch (e) {
        console.error('Błąd /api/historia', e);
    }
}

window.addEventListener('DOMContentLoaded', () => {
    utworzWykresProsty();

    pobierzStany();
    pobierzAlarmy();
    pobierzPaczki();
    odswiezWykres();
    tikajZegar();

    setInterval(pobierzStany, 2000);
    setInterval(pobierzAlarmy, 5000);
    setInterval(pobierzPaczki, 5000);
    setInterval(odswiezWykres, 3000);
    setInterval(tikajZegar, 1000);
});