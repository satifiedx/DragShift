//! English / Russian. The language follows Windows unless Settings pins one.
//! Strings are written in English in the code and looked up here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

static LANG: AtomicU8 = AtomicU8::new(0); // 0 = not decided yet, 1 = en, 2 = ru

fn system_is_russian() -> bool {
    let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    id & 0x3FF == 0x19
}

fn decide() -> u8 {
    match crate::settings::get().lang.as_str() {
        "ru" => 2,
        "en" => 1,
        _ => if system_is_russian() { 2 } else { 1 },
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
    if lang() == 2 { "ru" } else { "en" }
}

pub fn t<'a>(s: &'a str) -> &'a str {
    if lang() != 2 {
        return s;
    }
    ru().get(s).copied().unwrap_or(s)
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

#[cfg(test)]
mod tests {
    #[test]
    fn placeholders_match() {
        for (en, ru) in super::RU {
            assert_eq!(en.matches("{}").count(), ru.matches("{}").count(), "{en}");
        }
    }
}
