# WLED Matter (`wled_matter`)

[![Последний релиз](https://img.shields.io/github/v/release/dailrus/wled-matter?label=%D0%B2%D0%B5%D1%80%D1%81%D0%B8%D1%8F)](https://github.com/dailrus/wled-matter/releases/latest)
[![Скачать](https://img.shields.io/badge/%D1%81%D0%BA%D0%B0%D1%87%D0%B0%D1%82%D1%8C-%D0%BF%D0%B0%D0%BA%D0%B5%D1%82%20%D0%BF%D0%BE%D0%B4%20%D1%81%D0%B2%D0%BE%D1%8E%20%D0%BC%D0%BE%D0%B4%D0%B5%D0%BB%D1%8C-blue)](#совместимость)

Ленты [WLED](https://kno.wled.ge/) в «Доме с Алисой» без облака: Яндекс Станция сама
становится мостом и представляет каждую ленту как Matter-лампу — включение, яркость, цвет
и цветовая температура, голосом и из приложения, в том числе встроенные режимы Яндекса
(«Рассвет», «Закат», «Циркадный»).
Пакет для менеджера [yam](https://github.com/nyakokitsu/yam) — `dailrus/wled-matter`.

## Возможности

- Несколько лент: каждая — отдельное Matter-устройство со своим кодом привязки.
- Всё локально: команды идут Станция → лента по Wi-Fi, интернет не нужен.
- Состояние синхронизируется в обе стороны: если ленту переключили в приложении WLED или
  светомузыкой ([WLED Animation](https://github.com/dailrus/wled-anim)), «Дом с Алисой»
  это увидит.
- Веб-интерфейс на порту 8097 (и вкладка «WLED → Яндекс» в панели
  [Luna](https://github.com/nyakokitsu/luna), если стоит версия Luna с вкладками плагинов —
  в 0.2.3 их ещё нет, в 0.2.7 есть; плагин включается в Luna в разделе плагинов): ленты с
  проверкой и поиском по сети, QR-коды и коды привязки, сброс привязки.
- Работает в паре с [WLED Animation](https://github.com/dailrus/wled-anim): если она
  установлена, IP добавленной ленты (и число светодиодов) сразу уходит в неё — светомузыка
  играет на той же ленте без повторного ввода адреса. Подробнее — «Связь с WLED Animation».
- Лёгкий: один статический бинарник ~2 МБ, ~2 МБ памяти на Станции.
- Ставится на все модели, которые знает yam (см. «Совместимость»).

## Требования

- Яндекс Станция с root-доступом по adb и [yam](https://github.com/nyakokitsu/yam) ≥ 0.2.2
  (модели — в таблице ниже).
- Matter-контроллер Яндекса в доме, через который лента привязывается к «Дому с Алисой»:
  та же Станция или любая другая с поддержкой Matter. Мост сам по себе только «лампа»,
  ему контроллер не нужен — поэтому ставить его можно и на колонку без Matter.
- Лента на WLED в той же сети.

## Совместимость

yam проверяет платформу пакета при установке, поэтому для каждой модели — свой файл
`wled-matter-<платформа>.yampkg` (платформу показывает установщик yam и
`cat /data/yam/platforms/*.yaml`). Мост — статический бинарник, от прошивки он почти не
зависит; ему нужны Wi-Fi (или проводная сеть) и каталог `/data/yam`.

| Модель | Платформа yam | Бинарник | Проверено |
|---|---|---|---|
| Станция Мини 2 | `yandexmini_2` | aarch64 | ✓ |
| Станция Мини (1-е поколение) | `yandexmini` | armv7 | — |
| Станция Мини 3 | `bergamot` | aarch64 | — |
| Станция Мини 3 Про | `pickle` | aarch64 | — |
| Станция Лайт | `yandexmicro` | aarch64 | — |
| Станция Лайт 2 | `plum` (раньше `yandexplum`) | aarch64 | — |
| Станция Миди | `cucumber` | aarch64 | — |
| Станция 2 | `yandexmidi` | aarch64 | — |
| Станция 3 | `orion` | aarch64 | — |
| Станция Макс (в т. ч. с Zigbee) | `yandexstation_2` | aarch64 | — |
| Станция первого поколения | `yandexstation` | aarch64 | — |
| Умный хаб Яндекса | `saturn` | aarch64 | — |
| Станция Стрит | `mango` | armv7 | — |
| JBL Link Music | `jbl_link_music` | armv7 | — |

Модели и платформы — из документации yam (`docs/PLATFORMS.md`); у первой Мини yam пишет
aarch64, но Luna собирает для неё armv7, поэтому в пакете armv7-бинарник (он запускается и
на 64-битных ядрах). Сетевой интерфейс мост выбирает сам — тот, через который идёт маршрут
по умолчанию (переопределяется `MATTER_IFACE`). Если проверили на своей модели — напишите
в Issues, отмечу в таблице.

## Установка

Подставьте платформу своей модели из таблицы (пример — Мини 2):

```sh
P=yandexmini_2
curl -L -o wled-matter.yampkg https://github.com/dailrus/wled-matter/releases/latest/download/wled-matter-$P.yampkg
adb push wled-matter.yampkg /tmp/
adb shell /data/yam/bin/yam-run install --yes /tmp/wled-matter.yampkg
adb shell /data/yam/bin/wled_matter start
```

Обновление — те же команды: yam заменит установленную версию, ленты и привязки сохранятся.
`wled-matter.yampkg` без платформы в имени — пакет для Мини 2 (ссылка из версии 0.1.0).

### Автозапуск

yam сам службы не запускает, поэтому нужен init-скрипт:

```sh
adb push pkg/init/S99wled_matter /tmp/
adb shell "mount -o remount,rw / && cp /tmp/S99wled_matter /etc/init.d/ && chmod 755 /etc/init.d/S99wled_matter; mount -o remount,ro /"
```

## Привязка к «Дому с Алисой»

1. Откройте `http://<IP станции>:8097`, укажите IP ленты («Проверить» / «Найти в сети»)
   и нажмите «Сохранить» — у ленты появится QR-код и код привязки.
2. «Дом с Алисой» → «+» → «Устройство умного дома» → Matter → отсканируйте QR или
   введите код.

Устройство подписано тестовым сертификатом Matter SDK (производитель 0xFFF1) — Станция его
принимает, но это не сертифицированное CSA устройство.

## Связь с WLED Animation

Если на Станции стоит `dailrus/wled-anim`, мост сам держит её IP ленты в актуальном виде:

- добавили ленту — светомузыка переключается на неё;
- поменяли IP ленты, на которой играет светомузыка, — она следует за новым адресом;
- удалили эту ленту — светомузыка переходит на первую оставшуюся;
- в WLED Animation IP не задан (например, её поставили позже) — мост раз в минуту
  передаёт ей первую ленту.

В карточке ленты видно, на какую ленту идёт светомузыка; кнопка «Для светомузыки»
переключает её вручную. Мост пишет через API WLED Animation (порт 8098), а если её
веб-интерфейс не запущен — прямо в её файл настроек `/data/yam/var/wled_anim/settings`.

## Управление

- `wled_matter start | stop | restart | status | log [N]`.
- Ленты и привязки: `/data/yam/var/wled_matter` (`config.json` и по каталогу на ленту) —
  их же меняет веб-интерфейс. Удалите каталог ленты, и она получит новый код.
- Лог: `/tmp/wled_matter/bridge.log` (обрезается до 512 КБ).
- Порты: веб-интерфейс 8097/tcp; Matter 5540/udp и далее по одному на ленту; mDNS 5353/udp
  (общий со штатными службами).
- Удаление: `wled_matter stop`, затем `yam-run remove dailrus/wled-matter` и удалить
  `/etc/init.d/S99wled_matter`, если ставили автозапуск.

## Нагрузка на Станцию

Замер на Mini 2 с одной лентой, привязанной к «Дому с Алисой» (версия 0.1.0, armv7; сборка
aarch64 из 0.2.0 на той же колонке занимает ещё меньше — 1,3 МБ RSS без лент):

| Процесс | Память (PSS) | ЦП, % одного ядра |
|---|---|---|
| `wled_matterd` — мост (Matter, опрос ленты, веб-интерфейс) | 2,0 МБ (1,8 МБ RSS) | 0,2% |
| `sh` — перезапуск моста при падении | 0,1 МБ | 0% |
| **Итого** | **≈ 2,1 МБ** | **≈ 0,2% одного ядра ≈ 0,06% всего ЦП** (4 ядра) |

- Для сравнения: у Mini 2 244 МБ памяти, свободно обычно около 90–100 МБ.
- Каждая следующая лента добавляет ≈ 0,3 МБ (своё Matter-устройство) и ещё один опрос.
- Диск: пакет ≈ 2 МБ (бинарник), ленты и привязки ≈ 36 КБ, лог в `/tmp` (RAM) — до 512 КБ.
- Сеть: опрос ленты раз в 3 с (≈ 0,7 КБ на запрос, ≈ 0,25 КБ/с на ленту); команды и
  отчёты Matter — только при изменениях, плюс редкие mDNS-объявления.
- Замер ЦП — в простое за 60 с; во время команд из приложения мост кратко просыпается
  на каждую команду, а плавное изменение яркости даёт несколько запросов к ленте в секунду.

## Сборка

Мост написан на Rust поверх [rs-matter](https://github.com/project-chip/rs-matter) с двумя
правками для контроллера Яндекса (`bridge/patches/rs-matter.patch`, коммит rs-matter
`b88e772`): подписка не отклоняется целиком из-за необязательных путей, и General
Diagnostics отдаёт списки неисправностей.

```sh
git clone https://github.com/project-chip/rs-matter tools/rs-matter
git -C tools/rs-matter checkout b88e772730706d0ceb42b880f571b3dbc5e0573b
git -C tools/rs-matter apply ../../bridge/patches/rs-matter.patch
rustup target add armv7-unknown-linux-musleabihf
rustup target add aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf
# линковщик — zig cc: -target aarch64-linux-musl (без флага -Wl,--fix-cortex-a53-843419,
# см. scripts/zigwrap) и -target arm-linux-musleabihf -mcpu=cortex_a7 (scripts/zcc.bat); RUSTFLAGS: -C link-self-contained=no -C target-feature=+crt-static
cargo build --release --target aarch64-unknown-linux-musl --manifest-path bridge/Cargo.toml
cargo build --release --target armv7-unknown-linux-musleabihf --manifest-path bridge/Cargo.toml
```

Пакеты собирает `yam-publish` из тега `yam-v0.2.2` (более новый пишет в манифест поля,
которых yam 0.2.2 не знает) — по одному на платформу, с нужным бинарником в
`pkg/bin/wled_matterd`, например:

```sh
cp bridge/target/aarch64-unknown-linux-musl/release/wled_matter pkg/bin/wled_matterd
cp bridge/web/index.html pkg/luna/
yam-publish pack --source pkg --platform bergamot --profiles <каталог platforms из yam> --output dist/wled-matter-bergamot.yampkg
```

Весь набор под Windows собирают `scripts/build.ps1` и `scripts/pack.ps1` (в `tools/` должны
лежать портативные Rust, zig и yam-publish 0.2.2 — см. начало скриптов).

## Об авторстве

Проект сделан с помощью [Claude](https://claude.com/claude-code) (Claude Code,
Anthropic) по идее и под руководством [dailrus](https://github.com/dailrus).
Страницу интерфейса по заданию Claude написал DeepSeek; Claude её проверил и доработал.

## Лицензия

[MIT](LICENSE). Бинарник включает [rs-matter](https://github.com/project-chip/rs-matter)
(Apache-2.0).
