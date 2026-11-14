# GameOverlay

Оверлей поверх игр: показывает **FPS** и **пинг** до выбранного сервера.
Написан на Rust, использует только `windows` crate — без сторонних зависимостей.

![overlay](docs/overlay.png)

## Возможности

- Прозрачное окно поверх игры: кликабельность не блокируется (click-through), фокус не перехватывается
- FPS по счётчикам present'ов DWM (`cDXPresentSubmitted`) сглаживанием
- Пинг двумя методами: ICMP echo через raw-сокет (свой заголовок + контрольная сумма) и TCP-замер времени установления соединения (авто-откат, если ICMP режут)
- Несколько целей пинга с переключением по горячей клавише
- Оверлей следует за окном активной игры
- Установщик MSI (WiX 5), установка в профиль пользователя — права администратора не нужны

## Установка

**MSI** (рекомендуется): `GameOverlay-1.0.0-x64.msi` из релизов →
`%LOCALAPPDATA%\Programs\GameOverlay`, ярлык в меню «Пуск».

**Portable**: распаковать `GameOverlay-1.0.0-portable.zip` и запустить `GameOverlay.exe`.

## Использование

```
GameOverlay.exe                          # пинг до 1.1.1.1:443 (ICMP)
GameOverlay.exe --host game.example.com # конкретный сервер
GameOverlay.exe --port 25565            # порт для TCP-замера
GameOverlay.exe --targets "1.1.1.1:443,8.8.8.8:53"   # несколько целей
GameOverlay.exe --mode tcp              # только TCP-замер
GameOverlay.exe --fixed-position        # не следовать за окном игры
GameOverlay.exe --verbose               # печатать FPS/пинг в консоль
```

### Горячие клавиши

| Клавиша | Действие |
|---|---|
| `F1` | показать / скрыть оверлей |
| `F2` / `F3` | следующая / предыдущая цель пинга |
| `F4` | следовать за окном игры / фиксированная позиция |
| `End` | выход |

## Как считаются метрики

**FPS** — счётчики `cDXPresentSubmitted` / `cFrameSubmitted` из `DwmGetCompositionTimingInfo`.
Запрос по конкретному `HWND` на части сборок Windows возвращает `E_INVALIDARG`,
поэтому используется глобальный счётчик DWM: во время игры основной вклад даёт
сама игра.

**Пинг** — ICMP echo request через raw-сокет (`SOCK_RAW`, `IPPROTO_ICMP`):
заголовок ICMP (type 8, code 0, id, sequence) + контрольная сумма + payload,
ответ разбирается из `recvfrom` (в ответе Windows приходит полный IP-пакет,
ICMP-заголовок лежит за `IHL`-байтами IP-заголовка). RTT считается по
`QueryPerformanceCounter`. Если сеть блокирует ICMP — автоматический откат
на TCP-connect.

## Ограничения

- Оверлей отрисовывается GDI и виден в оконном / borderless-режиме. В
  эксклюзивном fullscreen его перекрывает сама игра — переключите игру в
  borderless (Alt+Enter) или оконный режим. Там же не растут счётчики DWM,
  поэтому FPS в эксклюзивном fullscreen не измеряется.
- ICMP через raw-сокет работает без повышения прав на Windows 10/11.

## Сборка из исходников

```powershell
# Rust toolchain: https://rustup.rs  (проверено на 1.100, target x86_64-pc-windows-gnu)
# WiX 5: https://wixtoolset.org
cd GameOverlay
cargo build --release
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

`build.ps1` собирает release-бинарь, генерирует иконку, упаковывает её в
`installer\stage\` и собирает MSI через WiX; на выходе в `dist\` появляются
`GameOverlay-1.0.0-x64.msi`, `GameOverlay-1.0.0-portable.zip` и `GameOverlay.exe`.

## Структура

```
src/main.rs      CLI, потоки замера, скрытие консоли
src/overlay.rs   окно оверлея, отрисовка, горячие клавиши, измерение FPS
src/ping.rs      ICMP (raw-сокет) и TCP-замер пинга
installer/       WiX-исходник установщика, иконка, README для пользователя
build.ps1        сборка exe + MSI + portable-архива
```

## Лицензия

MIT
