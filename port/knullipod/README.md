# KnulliPod

Un lettore musicale in stile iPod per le console Anbernic con Knulli, avviabile da PortMaster. Riproduce la musica in locale.

L'interfaccia è ricostruita in codice nativo a partire da [iPod.js](https://github.com/tvillarete/ipod-classic-js) di Tanner Villarete.

Sono riprodotti fedelmente split screen con anteprime e Ken Burns, liste, Now Playing con copertina in 3D, scrubber e volume, Cover Flow, Ricerca con tastiera a ghiera, menu a comparsa, About e i giochi Brick e Solitaire. Misure, colori, icone e tempi delle animazioni sono presi dal codice originale.

La riproduzione audio e la scansione della libreria derivano da [MiyooPod](https://github.com/danfragoso/miyoopod) di Danilo Fragoso. Entrambi i progetti sono in licenza MIT: vedi `LICENSE.ipod.js` e `LICENSE.MiyooPod`. Il font Inter è in licenza OFL. Il decoder Apple Lossless è quello di riferimento di Apple (Apache 2.0).

## Installazione

1. Copia `KnulliPod.sh` e la cartella `knullipod` in `/userdata/roms/ports/`.
2. Metti la musica in `/userdata/music`, che sulla rete si chiama `music`. Formati supportati: FLAC, ALAC e AAC (`.m4a` di iTunes / Apple Music, anche HE-AAC), WAV, AIFF, MP3, OGG. I `.m4a` protetti da DRM (vecchi acquisti iTunes) non si possono riprodurre. Organizzala in cartelle `Artista/Album/` con i tag compilati. Per i file senza tag (di solito WAV) artista, album e numero di traccia vengono presi dalle cartelle e dal nome del file. Le copertine incorporate nei file vengono lette in automatico (JPEG, PNG e WebP). Se un file non ne ha, viene usata un'immagine nella cartella dell'album (`cover.jpg`, `folder.jpg`, `front.png`…).
3. In EmulationStation aggiorna la lista dei giochi e avvia **KnulliPod** da Ports.

## Comandi

| Comando | Equivalente sull'iPod | Azione |
|---|---|---|
| Croce direzionale o stick (sinistro o destro) | Ghiera | Giù/destra avanti, su/sinistra indietro. Tenuto premuto continua a scorrere |
| A o click dello stick | Tasto centrale | Seleziona. Tenuto 0,5 s: opzioni (Play Next / Play Later) |
| B | MENU | Indietro. Tenuto 0,5 s: torna al menu principale |
| START o X | Play/Pausa | Play/Pausa |
| L / R | ⏮ / ⏭ | Brano precedente/successivo |
| Y | | Opzioni del brano o album selezionato |
| SELECT | Interruttore Hold | Blocca/sblocca i comandi e abbassa la luminosità. La musica continua |
| MENU (tasto funzione) | | Tocco breve: spegne lo schermo. Tenuto 5 s: esci |
| SELECT + START | | Esci |

Nella tastiera della Ricerca, L e R saltano di 6 tasti alla volta.

Nel Now Playing su/giù regolano il volume. Il tasto centrale attiva lo scrubber: a quel punto su/giù si spostano nel brano.

Lo schermo non si spegne da solo, come in ipod.js. Per spegnerlo a mano premi MENU (tasto funzione), e premilo di nuovo per riaccenderlo.

**Hold**: premi SELECT per mettere l'iPod in tasca. Tutti i comandi vengono ignorati (compare un lucchetto nella barra in alto), lo schermo si abbassa e la musica va avanti. In Hold, freccia su e giù regolano la luminosità (compare una barretta) e il valore scelto viene ricordato per la volta successiva; di base è un quinto della luminosità normale. Se premi un tasto per sbaglio compare il riquadro "Hold". Dopo 5 secondi in Hold si passa da soli al Now Playing, e al centro della barra in alto compare l'ora. Premi di nuovo SELECT per sbloccare. SELECT + START esce dall'app anche in Hold.

In **Solitaire** la ghiera sposta la mano. A prende o posa le carte; tenuto premuto annulla la mossa. B apre il menu di pausa (Resume, Undo, Redeal, Quit). La partita viene salvata.

In **Brick** sinistra/destra muovono la racchetta e A lancia la palla.

## Qualità audio

- Ogni brano viene riprodotto alla sua frequenza originale (44,1, 48, 96, 192 kHz…) e in 32 bit float: niente conversioni nell'app. FLAC e ALAC a 24 bit arrivano intatti al mixer di sistema.
- Il volume dell'app è il volume della console. Il segnale resta sempre al 100%, e i tasti volume della console o delle cuffie Bluetooth spostano anche la barra del volume nel Now Playing.

## Now Playing

- In pausa la copertina si raddrizza, e torna inclinata quando riparte la musica.
- Shuffle e Repeat sono indicati da icone nella barra in alto.
- Se cambi il volume dalla console o dalle cuffie fuori dal Now Playing, compare un riquadro col livello del volume.

## Cuffie

- I tasti delle cuffie funzionano, sia via cavo (se la console li riconosce) sia Bluetooth: play/pausa, brano successivo e precedente. Funzionano anche in Hold. Il volume delle cuffie Bluetooth è gestito dal sistema.
- Se stacchi il cavo delle cuffie o le cuffie Bluetooth si disconnettono, la musica va in pausa, come sull'iPod.
- Con cuffie Bluetooth collegate compare l'icona Bluetooth nella barra in alto.

## Opzioni (in `KnulliPod.sh`)

- `MIYOOPOD_SWAP_AB=0`: inverte A e B, se sulla tua console risultano scambiati.
- `MIYOOPOD_STICK_WHEEL=1`: usa lo stick ruotandolo come la ghiera dell'iPod, invece che come frecce. Girando veloce accelera.
- `MIYOOPOD_WHEEL_STEP=15`: con la ghiera attiva, gradi di rotazione per ogni scatto.
- `MIYOOPOD_HOLD_BRIGHTNESS=10`: luminosità in percentuale durante Hold, finché non la cambi con le frecce (di base un quinto di quella attuale).
- `MIYOOPOD_HOLD_ADJUST=0`: in Hold le frecce non cambiano la luminosità (Hold completamente inerte).
- `MIYOOPOD_SOFT_VOLUME=1`: usa il volume interno dell'app invece di quello della console.
- `MIYOOPOD_MEDIA_KEYS=0`: ignora i tasti delle cuffie.
- `MIYOOPOD_BT_ICON=0`: niente icona Bluetooth e niente pausa alla disconnessione.
- `MIYOOPOD_PERF=1`: scrive nel registro i tempi di disegno di ogni secondo (utile se qualcosa scatta).
- `MIYOOPOD_VSYNC=0`: disattiva la sincronizzazione verticale.
- `MIYOOPOD_CLASSIC_UI=1`: torna all'interfaccia originale di MiyooPod.

## Note

- La libreria, le impostazioni (volume, shuffle, ripetizione), le copertine e la partita di Solitaire stanno in `knullipod/conf`, a meno che non scegli un'altra cartella da Settings → Database.
- Settings → Database: sceglie la cartella del database (libreria, copertine, impostazioni, statistiche, salvataggi). Le scelte rapide sono:
  - la cartella del port (predefinita);
  - la cartella della musica (`/userdata/music/.knullipod`, nascosta, così la scansione la ignora);
  - una cartella `knullipod` su ogni scheda o disco montato in `/media`.

  Con "Choose a folder…" scegli una cartella qualsiasi, e lì puoi anche crearne una nuova chiamata `knullipod`.
  - Se nella cartella scelta non c'è un database, quello attuale viene spostato lì ("Move database here").
  - Se c'è già (per esempio una scheda usata su un'altra console), puoi usarlo così com'è ("Use database there") o sostituirlo con quello attuale ("Replace with current").

  La scelta resta salvata in `knullipod/conf/datadir.txt`. Se all'avvio la cartella non c'è (scheda tolta), KnulliPod usa la cartella del port e lo segnala nella stessa schermata.
- Il registro dell'app è in `knullipod/miyoopod.log`.
- Settings → Theme: chiaro (come ipod.js), scuro, oppure automatico (scuro dalle 20:00 alle 7:00).
- In Settings, Shuffle e Repeat aprono un sottomenu con le opzioni, e quella attiva ha la spunta.
- Settings → Library mostra la cartella della musica, quanti brani, album e artisti ha trovato, i formati, i file non supportati (per esempio AAC o Opus), lo spazio occupato e quello libero, e quando è stata fatta l'ultima scansione. Da lì si avvia "Rescan now".
- Gli album sono raggruppati per cartella e nome dell'album, quindi restano interi anche se i brani hanno artisti diversi o tag "album artist" mancanti. Le sottocartelle CD1, CD2, Disc 1… contano come un unico album.
- Da Settings non ci sono le voci di ipod.js che sulla console non hanno senso: servizio di streaming, accesso, tema del corpo dell'iPod e vibrazione.
