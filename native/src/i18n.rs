//! English / Russian / Ukrainian. The language follows Windows unless Settings pins one.
//! Strings are written in English in the code and looked up here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

static LANG: AtomicU8 = AtomicU8::new(0); // 0 = not decided yet, 1 = en, 2 = ru, 3 = uk

fn system_lang() -> u8 {
    let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    match id & 0x3FF {
        0x19 => 2,
        0x22 => 3,
        _ => 1,
    }
}

fn decide() -> u8 {
    match crate::settings::get().lang.as_str() {
        "ru" => 2,
        "uk" => 3,
        "en" => 1,
        _ => system_lang(),
    }
}

fn lang() -> u8 {
    match LANG.load(Ordering::Relaxed) {
        0 => {
            let l = decide();
            LANG.store(l, Ordering::Relaxed);
            l
        }
        l => l,
    }
}

/// Settings changed: decide again.
pub fn reload() {
    LANG.store(decide(), Ordering::Relaxed);
}

pub fn code() -> &'static str {
    match lang() {
        2 => "ru",
        3 => "uk",
        _ => "en",
    }
}

pub fn t<'a>(s: &'a str) -> &'a str {
    let table = match lang() {
        2 => ru(),
        3 => uk(),
        _ => return s,
    };
    table.get(s).copied().unwrap_or(s)
}

/// `t` with "{}" placeholders filled in order.
pub fn tf(fmt: &str, args: &[&str]) -> String {
    let f = t(fmt);
    let mut out = String::with_capacity(f.len() + 16);
    let mut it = args.iter();
    let mut rest = f;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(it.next().copied().unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

fn ru() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| RU.iter().copied().collect())
}

fn uk() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| UK.iter().copied().collect())
}

pub const RU: &[(&str, &str)] = &[
    // sizes
    ("B", "Б"),
    ("KB", "КБ"),
    ("MB", "МБ"),
    ("GB", "ГБ"),
    // rings and menu
    ("Convert", "Конвертация"),
    ("Tools", "Инструменты"),
    ("Compress", "Сжать"),
    ("Drop here", "Бросай сюда"),
    ("Can't convert {}", "Не умею {}"),
    ("this file", "этот файл"),
    ("Images · PDF · Word · Text · Video · Audio", "Фото · PDF · Word · Текст · Видео · Аудио"),
    ("{} files", "Файлов: {}"),
    ("{} images → 1 PDF", "{} фото → 1 PDF"),
    // format hints
    ("Small, universal", "Лёгкий, везде открывается"),
    ("Lossless · transparency", "Без потерь · прозрачность"),
    ("Smallest for web", "Самый лёгкий для веба"),
    ("iPhone format", "Формат iPhone"),
    ("Print · archival", "Печать · архив"),
    ("Next-gen, tiny", "Новый, крошечный"),
    ("Uncompressed", "Без сжатия"),
    ("Document", "Документ"),
    ("Windows icon", "Иконка Windows"),
    ("Animated · legacy web", "Анимация · старый веб"),
    ("Editable Word", "Редактируемый Word"),
    ("Plain text", "Просто текст"),
    ("Plays everywhere", "Играет везде"),
    ("Keeps every track", "Хранит все дорожки"),
    ("Open web video", "Открытое веб-видео"),
    ("Apple · editing", "Apple · монтаж"),
    ("Old players", "Старые плееры"),
    ("Universal audio", "Универсальный звук"),
    ("AAC · Apple, phones", "AAC · Apple, телефоны"),
    ("Uncompressed audio", "Звук без сжатия"),
    ("Lossless, smaller", "Без потерь, легче"),
    ("Vorbis · games, web", "Vorbis · игры, веб"),
    ("Best quality per MB", "Лучшее качество на МБ"),
    ("Uncompressed · Mac", "Без сжатия · Mac"),
    ("Windows Media", "Windows Media"),
    ("Lossless · Apple", "Без потерь · Apple"),
    // tools
    ("Resize", "Размер"),
    ("Crop", "Кадрировать"),
    ("Edit", "Правка"),
    ("Annotate", "Пометки"),
    ("Redact", "Скрыть"),
    ("Add BG", "Фон"),
    ("Metadata", "Метаданные"),
    ("Split", "Разделить"),
    ("Rotate", "Повернуть"),
    ("Merge", "Объединить"),
    ("Protect", "Пароль"),
    ("Extract", "Картинки"),
    ("Trim", "Обрезать"),
    ("Speed", "Скорость"),
    ("Mute", "Без звука"),
    ("Audio", "Аудио"),
    ("Frame", "Стоп-кадр"),
    ("Shrink file size", "Уменьшить вес"),
    ("Change dimensions", "Изменить размеры"),
    ("Trim to a ratio", "Обрезать под формат"),
    ("Light · colour · rotate", "Свет · цвет · поворот"),
    ("Arrows · text · steps", "Стрелки · текст · шаги"),
    ("Hide private info", "Спрятать личное"),
    ("Gradient backdrop", "Градиентный фон"),
    ("View · remove GPS", "Смотреть · убрать GPS"),
    ("One PDF per page", "PDF на каждую страницу"),
    ("Turn pages 90°", "Повернуть страницы на 90°"),
    ("Join into one PDF", "Собрать в один PDF"),
    ("Add a password", "Поставить пароль"),
    ("Save embedded images", "Достать картинки"),
    ("Cut start and end", "Срезать начало и конец"),
    ("Cut the frame", "Обрезать кадр"),
    ("90° clockwise", "90° по часовой"),
    ("Faster or slower", "Быстрее или медленнее"),
    ("Remove the sound", "Убрать звук"),
    ("Save the soundtrack", "Вытащить звук"),
    ("Snapshot as PNG", "Снимок в PNG"),
    ("Waveform · fades", "Волна · затухания"),
    ("Discord, email", "Discord, почта"),
    ("Email, messengers", "Почта, мессенджеры"),
    ("Messengers", "Мессенджеры"),
    ("Uploads", "Загрузки"),
    ("Half the size", "В два раза легче"),
    ("A quarter of the size", "В четыре раза легче"),
    ("Best MP3 quality", "Максимум для MP3"),
    ("Near transparent", "Почти без потерь"),
    ("Great for music", "Отлично для музыки"),
    ("Good · small", "Норм · лёгкий"),
    ("Podcasts, voice", "Подкасты, голос"),
    ("Voice, smallest", "Голос, самый лёгкий"),
    // working
    ("Converting to {}…", "Конвертирую в {}…"),
    ("Compressing to {} MB…", "Сжимаю до {} МБ…"),
    ("Compressing…", "Сжимаю…"),
    ("Rotating…", "Поворачиваю…"),
    ("Removing the sound…", "Убираю звук…"),
    ("Saving the soundtrack…", "Вытаскиваю звук…"),
    ("Saving a frame…", "Сохраняю кадр…"),
    ("Trimming…", "Обрезаю…"),
    ("Cropping…", "Кадрирую…"),
    ("Changing the speed…", "Меняю скорость…"),
    ("Opening {}…", "Открываю «{}»…"),
    ("Working…", "Работаю…"),
    ("Cancelling…", "Отменяю…"),
    ("File {} of {}", "Файл {} из {}"),
    // results
    ("Nothing was written", "Ничего не записалось"),
    ("{} files → {}", "{} файлов → {}"),
    ("Compressed", "Сжато"),
    ("Compressed {} files", "Сжато файлов: {}"),
    ("already well compressed", "уже хорошо сжат"),
    ("{}% smaller", "на {}% легче"),
    ("Rotated 90°", "Повёрнуто на 90°"),
    ("Sound removed", "Звук убран"),
    ("Soundtrack saved", "Звук сохранён"),
    ("Frame saved", "Кадр сохранён"),
    ("Trimmed", "Обрезано"),
    ("Cropped", "Кадрировано"),
    ("Speed changed", "Скорость изменена"),
    ("Done", "Готово"),
    ("Rotated", "Повёрнуто"),
    ("Muted", "Без звука"),
    ("{} didn't work", "не вышло: {}"),
    ("Cancelled", "Отменено"),
    ("Nothing was changed.", "Ничего не изменилось."),
    ("Couldn't finish", "Не получилось"),
    // toast buttons
    ("Open", "Открыть"),
    ("Show in folder", "В папке"),
    ("Undo", "Отменить"),
    ("Cancel", "Стоп"),
    ("Undone", "Отменено"),
    ("Moved 1 file to the Recycle Bin", "1 файл ушёл в корзину"),
    ("Moved {} files to the Recycle Bin", "Файлов в корзине: {}"),
    // errors
    ("Video and audio need ffmpeg, and DragShift couldn't find it. Reinstall DragShift.",
     "Для видео и аудио нужен ffmpeg, а DragShift его не нашёл. Переустанови DragShift."),
    ("ffmpeg is missing", "Нет ffmpeg"),
    ("This file looks damaged, or it isn't a video or audio format DragShift can read.",
     "Файл битый или это формат, который DragShift не читает."),
    ("The file was moved or deleted.", "Файл переместили или удалили."),
    ("This copy of ffmpeg can't make {} (it has no {} encoder).", "Этот ffmpeg не умеет {} (нет кодировщика {})."),
    ("The disk is full.", "Диск забит."),
    ("No permission to write next to this file. Try a folder you own, or set Settings → Save results → subfolder.",
     "Нет прав писать рядом с файлом. Возьми свою папку или включи в настройках сохранение в подпапку."),
    ("ffmpeg couldn't process this file.", "ffmpeg не справился с этим файлом."),
    ("ffmpeg finished but didn't write anything.", "ffmpeg отработал, но ничего не записал."),
    ("This file has no video, only sound.", "Тут нет видео, только звук."),
    ("This video has no sound.", "В этом видео нет звука."),
    ("It's {}, already under {}.", "Тут {}, это и так меньше {}."),
    ("Nothing to do", "Делать нечего"),
    ("It's already {} kbps. A higher bitrate would only make the file bigger, not better.",
     "Тут и так {} kbps. Битрейт выше только раздует файл, лучше звучать не станет."),
    ("This clip is {} long. GIFs work best under {} seconds, so trim it first.",
     "Ролик длиной {}. GIF норм до {} секунд, сначала обрежь."),
    ("DragShift can't tell how long this file is, so it can't aim for a size.",
     "Непонятна длина файла, поэтому под размер не сжать."),
    ("This video is too long to fit in {}. Trim it first or pick a bigger size.",
     "Видео слишком длинное для {}. Обрежь или выбери размер побольше."),
    ("Pick a start point before the end point.", "Начало должно быть раньше конца."),
    ("The crop area is outside the video.", "Рамка вылезает за кадр."),
    ("Speed must be between 0.25× and 4×.", "Скорость от 0.25× до 4×."),
    ("Couldn't start the editor", "Редактор не запустился"),
    ("Part of DragShift is missing. Reinstall DragShift.", "Не хватает части DragShift. Переустанови его."),
    ("The editor closed unexpectedly. Please try again.", "Редактор внезапно закрылся. Попробуй ещё раз."),
    // app and tray
    ("DragShift is ready", "DragShift готов"),
    ("Start dragging any file, then press {}. The menu opens under the cursor.",
     "Начни тащить любой файл и нажми {}. Меню откроется под курсором."),
    ("DragShift is running", "DragShift работает"),
    ("Drag a file and press {}. Settings are in the tray icon.", "Тащи файл и жми {}. Настройки в иконке в трее."),
    ("DragShift is paused", "DragShift на паузе"),
    ("Turn it back on from the tray icon.", "Включить обратно можно в трее."),
    ("DragShift is on", "DragShift включён"),
    ("Drag a file and press {}.", "Тащи файл и жми {}."),
    ("How to use DragShift", "Как пользоваться"),
    ("Start dragging a file, press {} and drop it on a petal. Press {} again for more options.",
     "Начни тащить файл, нажми {} и брось на лепесток. Ещё раз {} для других действий."),
    ("Enabled", "Включён"),
    ("Open a file…", "Открыть файл…"),
    ("Settings…", "Настройки…"),
    ("How to use", "Как пользоваться"),
    ("About DragShift", "О DragShift"),
    ("Quit", "Выход"),
    ("paused", "пауза"),
    ("All supported files", "Все файлы"),
    ("Choose files for DragShift", "Выбери файлы для DragShift"),
];

pub const UK: &[(&str, &str)] = &[
    // sizes
    ("B", "Б"),
    ("KB", "КБ"),
    ("MB", "МБ"),
    ("GB", "ГБ"),
    // rings and menu
    ("Convert", "Конвертація"),
    ("Tools", "Інструменти"),
    ("Compress", "Стиснути"),
    ("Drop here", "Кидай сюди"),
    ("Can't convert {}", "Не вмію {}"),
    ("this file", "цей файл"),
    ("Images · PDF · Word · Text · Video · Audio", "Фото · PDF · Word · Текст · Відео · Аудіо"),
    ("{} files", "Файлів: {}"),
    ("{} images → 1 PDF", "{} фото → 1 PDF"),
    // format hints
    ("Small, universal", "Легкий, відкривається всюди"),
    ("Lossless · transparency", "Без втрат · прозорість"),
    ("Smallest for web", "Найлегший для вебу"),
    ("iPhone format", "Формат iPhone"),
    ("Print · archival", "Друк · архів"),
    ("Next-gen, tiny", "Новий, крихітний"),
    ("Uncompressed", "Без стиснення"),
    ("Document", "Документ"),
    ("Windows icon", "Іконка Windows"),
    ("Animated · legacy web", "Анімація · старий веб"),
    ("Editable Word", "Редагований Word"),
    ("Plain text", "Просто текст"),
    ("Plays everywhere", "Грає всюди"),
    ("Keeps every track", "Зберігає всі доріжки"),
    ("Open web video", "Відкрите веб-відео"),
    ("Apple · editing", "Apple · монтаж"),
    ("Old players", "Старі плеєри"),
    ("Universal audio", "Універсальний звук"),
    ("AAC · Apple, phones", "AAC · Apple, телефони"),
    ("Uncompressed audio", "Звук без стиснення"),
    ("Lossless, smaller", "Без втрат, легший"),
    ("Vorbis · games, web", "Vorbis · ігри, веб"),
    ("Best quality per MB", "Найкраща якість на МБ"),
    ("Uncompressed · Mac", "Без стиснення · Mac"),
    ("Windows Media", "Windows Media"),
    ("Lossless · Apple", "Без втрат · Apple"),
    // tools
    ("Resize", "Розмір"),
    ("Crop", "Кадрувати"),
    ("Edit", "Правка"),
    ("Annotate", "Позначки"),
    ("Redact", "Приховати"),
    ("Add BG", "Тло"),
    ("Metadata", "Метадані"),
    ("Split", "Розділити"),
    ("Rotate", "Повернути"),
    ("Merge", "Об'єднати"),
    ("Protect", "Пароль"),
    ("Extract", "Картинки"),
    ("Trim", "Обрізати"),
    ("Speed", "Швидкість"),
    ("Mute", "Без звуку"),
    ("Audio", "Аудіо"),
    ("Frame", "Стоп-кадр"),
    ("Shrink file size", "Зменшити вагу"),
    ("Change dimensions", "Змінити розміри"),
    ("Trim to a ratio", "Обрізати під формат"),
    ("Light · colour · rotate", "Світло · колір · поворот"),
    ("Arrows · text · steps", "Стрілки · текст · кроки"),
    ("Hide private info", "Сховати особисте"),
    ("Gradient backdrop", "Градієнтне тло"),
    ("View · remove GPS", "Дивитися · прибрати GPS"),
    ("One PDF per page", "PDF на кожну сторінку"),
    ("Turn pages 90°", "Повернути сторінки на 90°"),
    ("Join into one PDF", "Зібрати в один PDF"),
    ("Add a password", "Поставити пароль"),
    ("Save embedded images", "Дістати картинки"),
    ("Cut start and end", "Зрізати початок і кінець"),
    ("Cut the frame", "Обрізати кадр"),
    ("90° clockwise", "90° за годинниковою"),
    ("Faster or slower", "Швидше або повільніше"),
    ("Remove the sound", "Прибрати звук"),
    ("Save the soundtrack", "Витягти звук"),
    ("Snapshot as PNG", "Знімок у PNG"),
    ("Waveform · fades", "Хвиля · згасання"),
    ("Discord, email", "Discord, пошта"),
    ("Email, messengers", "Пошта, месенджери"),
    ("Messengers", "Месенджери"),
    ("Uploads", "Завантаження"),
    ("Half the size", "Удвічі легший"),
    ("A quarter of the size", "Учетверо легший"),
    ("Best MP3 quality", "Максимум для MP3"),
    ("Near transparent", "Майже без втрат"),
    ("Great for music", "Чудово для музики"),
    ("Good · small", "Норм · легкий"),
    ("Podcasts, voice", "Подкасти, голос"),
    ("Voice, smallest", "Голос, найлегший"),
    // working
    ("Converting to {}…", "Конвертую в {}…"),
    ("Compressing to {} MB…", "Стискаю до {} МБ…"),
    ("Compressing…", "Стискаю…"),
    ("Rotating…", "Повертаю…"),
    ("Removing the sound…", "Прибираю звук…"),
    ("Saving the soundtrack…", "Витягаю звук…"),
    ("Saving a frame…", "Зберігаю кадр…"),
    ("Trimming…", "Обрізаю…"),
    ("Cropping…", "Кадрую…"),
    ("Changing the speed…", "Змінюю швидкість…"),
    ("Opening {}…", "Відкриваю «{}»…"),
    ("Working…", "Працюю…"),
    ("Cancelling…", "Скасовую…"),
    ("File {} of {}", "Файл {} з {}"),
    // results
    ("Nothing was written", "Нічого не записалося"),
    ("{} files → {}", "{} файлів → {}"),
    ("Compressed", "Стиснуто"),
    ("Compressed {} files", "Стиснуто файлів: {}"),
    ("already well compressed", "уже добре стиснутий"),
    ("{}% smaller", "на {}% легше"),
    ("Rotated 90°", "Повернуто на 90°"),
    ("Sound removed", "Звук прибрано"),
    ("Soundtrack saved", "Звук збережено"),
    ("Frame saved", "Кадр збережено"),
    ("Trimmed", "Обрізано"),
    ("Cropped", "Кадровано"),
    ("Speed changed", "Швидкість змінено"),
    ("Done", "Готово"),
    ("Rotated", "Повернуто"),
    ("Muted", "Без звуку"),
    ("{} didn't work", "не вийшло: {}"),
    ("Cancelled", "Скасовано"),
    ("Nothing was changed.", "Нічого не змінилося."),
    ("Couldn't finish", "Не вдалося"),
    // toast buttons
    ("Open", "Відкрити"),
    ("Show in folder", "У теці"),
    ("Undo", "Скасувати"),
    ("Cancel", "Стоп"),
    ("Undone", "Скасовано"),
    ("Moved 1 file to the Recycle Bin", "1 файл пішов у кошик"),
    ("Moved {} files to the Recycle Bin", "Файлів у кошику: {}"),
    // errors
    ("Video and audio need ffmpeg, and DragShift couldn't find it. Reinstall DragShift.",
     "Для відео й аудіо потрібен ffmpeg, а DragShift його не знайшов. Перевстанови DragShift."),
    ("ffmpeg is missing", "Немає ffmpeg"),
    ("This file looks damaged, or it isn't a video or audio format DragShift can read.",
     "Файл пошкоджений або це формат, який DragShift не читає."),
    ("The file was moved or deleted.", "Файл перемістили або видалили."),
    ("This copy of ffmpeg can't make {} (it has no {} encoder).", "Цей ffmpeg не вміє {} (немає кодувальника {})."),
    ("The disk is full.", "Диск заповнений."),
    ("No permission to write next to this file. Try a folder you own, or set Settings → Save results → subfolder.",
     "Немає прав писати поруч із файлом. Візьми свою теку або ввімкни в налаштуваннях збереження в підтеку."),
    ("ffmpeg couldn't process this file.", "ffmpeg не впорався з цим файлом."),
    ("ffmpeg finished but didn't write anything.", "ffmpeg відпрацював, але нічого не записав."),
    ("This file has no video, only sound.", "Тут немає відео, лише звук."),
    ("This video has no sound.", "У цьому відео немає звуку."),
    ("It's {}, already under {}.", "Тут {}, це й так менше {}."),
    ("Nothing to do", "Нічого робити"),
    ("It's already {} kbps. A higher bitrate would only make the file bigger, not better.",
     "Тут і так {} kbps. Вищий бітрейт лише роздує файл, краще звучати не стане."),
    ("This clip is {} long. GIFs work best under {} seconds, so trim it first.",
     "Ролик довжиною {}. GIF норм до {} секунд, спершу обріж."),
    ("DragShift can't tell how long this file is, so it can't aim for a size.",
     "Незрозуміла довжина файлу, тому під розмір не стиснути."),
    ("This video is too long to fit in {}. Trim it first or pick a bigger size.",
     "Відео задовге для {}. Обріж або вибери більший розмір."),
    ("Pick a start point before the end point.", "Початок має бути раніше за кінець."),
    ("The crop area is outside the video.", "Рамка вилазить за кадр."),
    ("Speed must be between 0.25× and 4×.", "Швидкість від 0.25× до 4×."),
    ("Couldn't start the editor", "Редактор не запустився"),
    ("Part of DragShift is missing. Reinstall DragShift.", "Бракує частини DragShift. Перевстанови його."),
    ("The editor closed unexpectedly. Please try again.", "Редактор раптово закрився. Спробуй ще раз."),
    // app and tray
    ("DragShift is ready", "DragShift готовий"),
    ("Start dragging any file, then press {}. The menu opens under the cursor.",
     "Почни тягнути будь-який файл і натисни {}. Меню відкриється під курсором."),
    ("DragShift is running", "DragShift працює"),
    ("Drag a file and press {}. Settings are in the tray icon.", "Тягни файл і тисни {}. Налаштування в іконці в треї."),
    ("DragShift is paused", "DragShift на паузі"),
    ("Turn it back on from the tray icon.", "Увімкнути назад можна в треї."),
    ("DragShift is on", "DragShift увімкнено"),
    ("Drag a file and press {}.", "Тягни файл і тисни {}."),
    ("How to use DragShift", "Як користуватися"),
    ("Start dragging a file, press {} and drop it on a petal. Press {} again for more options.",
     "Почни тягнути файл, натисни {} і кинь на пелюстку. Ще раз {} для інших дій."),
    ("Enabled", "Увімкнено"),
    ("Open a file…", "Відкрити файл…"),
    ("Settings…", "Налаштування…"),
    ("How to use", "Як користуватися"),
    ("About DragShift", "Про DragShift"),
    ("Quit", "Вихід"),
    ("paused", "пауза"),
    ("All supported files", "Усі файли"),
    ("Choose files for DragShift", "Вибери файли для DragShift"),
];

#[cfg(test)]
mod tests {
    #[test]
    fn placeholders_match() {
        for (en, tr) in super::RU.iter().chain(super::UK) {
            assert_eq!(en.matches("{}").count(), tr.matches("{}").count(), "{en}");
        }
    }

    #[test]
    fn ukrainian_covers_russian() {
        let ru: std::collections::BTreeSet<_> = super::RU.iter().map(|p| p.0).collect();
        let uk: std::collections::BTreeSet<_> = super::UK.iter().map(|p| p.0).collect();
        assert_eq!(ru, uk);
    }
}
