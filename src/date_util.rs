use chrono::{Duration, Local, NaiveDate, NaiveDateTime, Timelike};

/// Serial date epoch = 1899-12-30 (Power BI / OLE Automation convention).
///
/// This uses **clean proleptic Gregorian** dates — there is no fake
/// 1900-02-29 like in Excel. The mapping is:
///
/// | Date       | Serial | vs Excel               |
/// |------------|--------|------------------------|
/// | 1899-12-30 | 0      | n/a (Excel starts at 1)|
/// | 1899-12-31 | 1      | n/a                    |
/// | 1900-01-01 | 2      | Excel: 1 (off by 1)    |
/// | 1900-02-28 | 60     | Excel: 59 (off by 1)   |
/// | 1900-03-01 | 61     | **matches Excel**       |
/// | 2024-01-01 | 45292  | **matches Excel**       |
///
/// All dates from 1900-03-01 onward match Excel exactly. For January /
/// February 1900 Excel's value is 1 lower because Excel pretends 1900-02-29
/// existed; we don't, so weekday math is also correct year-round (Excel's
/// WEEKDAY is off in January / February 1900). This is the same convention
/// Power BI uses.
fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1899, 12, 30).unwrap()
}

pub fn date_to_serial(date: NaiveDate) -> f64 {
    (date - epoch()).num_days() as f64
}

pub fn serial_to_date(serial: f64) -> Option<NaiveDate> {
    let days = serial.floor() as i64;
    epoch().checked_add_signed(Duration::days(days))
}

pub fn datetime_to_serial(dt: NaiveDateTime) -> f64 {
    let day_part = (dt.date() - epoch()).num_days() as f64;
    let frac = (dt.time().hour() as f64 * 3600.0
        + dt.time().minute() as f64 * 60.0
        + dt.time().second() as f64)
        / 86400.0;
    day_part + frac
}

pub fn today_serial() -> f64 {
    date_to_serial(Local::now().date_naive())
}

pub fn now_serial() -> f64 {
    datetime_to_serial(Local::now().naive_local())
}
