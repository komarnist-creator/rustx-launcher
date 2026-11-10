rustx - Minecraft launcher (Rust)

КАК ПОЛУЧИТЬ rustx.exe (всё делает GitHub, ставить Rust не нужно)
1. Распакуй архив и залей ВСЕ файлы в репозиторий (Add file -> Upload files).
2. Файл сборки должен лежать в .github/workflows/main.yml (в архиве он уже там).
   Если папка .github не загрузилась: Actions -> Workflow file -> карандаш -> вставь
   содержимое файла build.yml из архива -> Commit changes.
3. Через 4-5 минут на главной странице репозитория справа в Releases
   появится "rustx (latest)" - там лежит rustx.exe.
   Если ошибка про права: Settings -> Actions -> General -> Workflow permissions -> Read and write.

rustx - лаунчер Minecraft на Rust
=================================
Всё, что было в Python-версии: Vanilla / Fabric / Forge / Quilt, офлайн и Ely.by (скины),
выбор версии, ОЗУ игре, настройки, очистка логов, ярлык на рабочем столе, F11 - полный экран.
Конфиг тот же: %APPDATA%\.minecraft\rustx.json (старые настройки подхватятся).

КАК ПОЛУЧИТЬ rustx.exe БЕЗ УСТАНОВКИ RUST (через GitHub)
1. Создай репозиторий на GitHub и загрузи в него все файлы из этой папки (Add file -> Upload files).
2. Файл build.yml должен лежать по пути .github/workflows/build.yml. Как переместить:
   открой build.yml в репозитории -> карандаш (Edit) -> в поле имени поставь курсор в самое начало
   и допиши .github/workflows/ перед build.yml -> Commit changes.
3. Сборка запустится сама (или Actions -> build-exe -> Run workflow).
4. Через 5-10 минут справа на главной странице в Releases появится "rustx (latest)" - там лежит чистый rustx.exe (не zip).
   Прямая ссылка: https://github.com/ТВОЙ_НИК/rustx/releases/latest/download/rustx.exe
   Если сборка падает на правах: Settings -> Actions -> General -> Workflow permissions -> Read and write permissions.

КАК СОБРАТЬ ЛОКАЛЬНО
1. Поставь Rust: https://rustup.rs (нужны Visual Studio Build Tools с C++).
2. Запусти build_exe.bat -> появится rustx.exe.

СТРУКТУРА
main.rs, engine.rs, sys.rs - исходники на Rust
ram.c - маленький C-модуль (чтение ОЗУ), собирается автоматически через build.rs

ФОН: Настройки -> Фон лаунчера: Авто фон / Без фона.

ФОН: Настройки -> Фон лаунчера: Авто фон (картинка по умолчанию) / Свой фон (твоя картинка PNG или JPG на всё окно) / Без фона (просто фиолетовый).
Своя картинка грузится в фоне, поэтому лаунчер открывается так же быстро.
