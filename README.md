# WLED Matter (`wled_matter`)

[![Последний релиз](https://img.shields.io/github/v/release/dailrus/wled-matter?label=%D0%B2%D0%B5%D1%80%D1%81%D0%B8%D1%8F)](https://github.com/dailrus/wled-matter/releases/latest)
[![Скачать](https://img.shields.io/badge/%D1%81%D0%BA%D0%B0%D1%87%D0%B0%D1%82%D1%8C-wled--matter.yampkg-blue)](https://github.com/dailrus/wled-matter/releases/latest/download/wled-matter.yampkg)

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

## Требования

- Яндекс Станция Mini 2 (платформа `yandexmini_2`) с root-доступом по adb.
- [yam](https://github.com/nyakokitsu/yam) ≥ 0.2.2 на колонке.
- Лента на WLED в той же сети.

## Установка

```sh
curl -L -o wled-matter.yampkg https://github.com/dailrus/wled-matter/releases/latest/download/wled-matter.yampkg
adb push wled-matter.yampkg /tmp/
adb shell /data/yam/bin/yam-run install --yes /tmp/wled-matter.yampkg
adb shell /data/yam/bin/wled_matter start
```

Обновление — те же команды: yam заменит установленную версию, ленты и привязки сохранятся.

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

Замер на Mini 2 с одной лентой, привязанной к «Дому с Алисой» (версия 0.1.0):

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
# линковщик — zig cc (-target arm-linux-musleabihf -mcpu=cortex_a7), см. tools/zcc.bat
cargo build --release --target armv7-unknown-linux-musleabihf --manifest-path bridge/Cargo.toml
cp bridge/target/armv7-unknown-linux-musleabihf/release/wled_matter pkg/bin/wled_matterd
cp bridge/web/index.html pkg/luna/
yam-publish pack --source pkg --platform yandexmini_2 --profiles <каталог platforms из yam> --output dist/wled-matter-<версия>.yampkg
```

В релиз кладутся два одинаковых файла: `wled-matter-<версия>.yampkg` и
`wled-matter.yampkg` — на второй ведёт постоянная ссылка «последний релиз».

## Об авторстве

Проект сделан с помощью [Claude](https://claude.com/claude-code) (Claude Code,
Anthropic) по идее и под руководством [dailrus](https://github.com/dailrus).
Страницу интерфейса по заданию Claude написал DeepSeek; Claude её проверил и доработал.

## Лицензия

[MIT](LICENSE). Бинарник включает [rs-matter](https://github.com/project-chip/rs-matter)
(Apache-2.0).
