// #![cfg(feature = "chrono")]

// //! Conversions to and from [chrono](https://docs.rs/chrono/)’s `Duration`,
// //! `Date`, `Time`, `OffsetDateTime`, `FixedOffset`, and `Utc`.
// //!
// //! # Setup
// //!
// //! To use this feature, add this to your **`Cargo.toml`**:
// //!
// //! ```toml
// //! [dependencies]
// //! chrono = "0.4"
// #![doc = concat!("pyo3 = { version = \"", env!("CARGO_PKG_VERSION"),  "\", features = [\"chrono\"] }")]
// //! ```
// //!
// //! Note that you must use compatible versions of chrono and PyO3.
// //! The required chrono version may vary based on the version of PyO3.
// //!
// //! # Example: Convert a `datetime.datetime` to chrono's `OffsetDateTime<Utc>`
// //!
// //! ```rust
// //! use chrono::{OffsetDateTime, Duration, TimeZone, Utc};
// //! use pyo3::{Python, PyResult, IntoPyObject, types::PyAnyMethods};
// //!
// //! fn main() -> PyResult<()> {
// //!     pyo3::prepare_freethreaded_python();
// //!     Python::with_gil(|py| {
// //!         // Build some chrono values
// //!         let chrono_datetime = Utc.with_ymd_and_hms(2022, 1, 1, 12, 0, 0).unwrap();
// //!         let chrono_duration = Duration::seconds(1);
// //!         // Convert them to Python
// //!         let py_datetime = chrono_datetime.into_pyobject(py)?;
// //!         let py_timedelta = chrono_duration.into_pyobject(py)?;
// //!         // Do an operation in Python
// //!         let py_sum = py_datetime.call_method1("__add__", (py_timedelta,))?;
// //!         // Convert back to Rust
// //!         let chrono_sum: OffsetDateTime<Utc> = py_sum.extract()?;
// //!         println!("OffsetDateTime<Utc>: {}", chrono_datetime);
// //!         Ok(())
// //!     })
// //! }
// //! ```

use crate::conversion::IntoPyObject;
use crate::exceptions::{PyTypeError, PyUserWarning, PyValueError};
#[cfg(Py_LIMITED_API)]
use crate::sync::GILOnceCell;
use crate::types::any::PyAnyMethods;
#[cfg(not(Py_LIMITED_API))]
use crate::types::datetime::timezone_from_offset;
#[cfg(not(Py_LIMITED_API))]
use crate::types::{
    PyDate, PyDateAccess, PyDateTime, PyDelta, PyDeltaAccess, PyTime, PyTimeAccess, PyTzInfo,
    PyTzInfoAccess,
};
use crate::types::{PyInt, PyNone};
use crate::{ffi, Bound, FromPyObject, PyAny, PyErr, PyObject, PyResult, Python};
#[cfg(Py_LIMITED_API)]
use crate::{intern, DowncastError};
#[allow(deprecated)]
use crate::{IntoPy, ToPyObject};

// use chrono::{
//     offset::{FixedOffset, Utc},
//     DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Timelike,
// };

use time::OffsetDateTime;
use time::PrimitiveDateTime;
use time::Time;
use time::{Date, UtcOffset};
use time::{Duration, Month};

#[allow(deprecated)]
impl ToPyObject for Duration {
    #[inline]
    fn to_object(&self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

#[allow(deprecated)]
impl IntoPy<PyObject> for Duration {
    #[inline]
    fn into_py(self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

impl<'py> IntoPyObject<'py> for Duration {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyDelta;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        // Total number of days
        let days = self.whole_days();
        // Remainder of seconds
        let secs_dur = self - Duration::days(days);
        let secs = secs_dur.whole_seconds();
        // Fractional part of the microseconds
        let micros = (secs_dur - Duration::seconds(secs_dur.whole_seconds())).whole_microseconds();
        // This should never panic since we are just getting the fractional
        // part of the total microseconds, which should never overflow.
        // .unwrap();

        #[cfg(not(Py_LIMITED_API))]
        {
            // We do not need to check the days i64 to i32 cast from rust because
            // python will panic with OverflowError.
            // We pass true as the `normalize` parameter since we'd need to do several checks here to
            // avoid that, and it shouldn't have a big performance impact.
            // The seconds and microseconds cast should never overflow since it's at most the number of seconds per day
            PyDelta::new(
                py,
                days.try_into().unwrap_or(i32::MAX),
                secs.try_into()?,
                micros.try_into()?,
                true,
            )
        }

        #[cfg(Py_LIMITED_API)]
        {
            DatetimeTypes::try_get(py)
                .and_then(|dt| dt.timedelta.bind(py).call1((days, secs, micros)))
        }
    }
}

impl<'py> IntoPyObject<'py> for &Duration {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyDelta;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    #[inline]
    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        (*self).into_pyobject(py)
    }
}

impl FromPyObject<'_> for Month {
    fn extract_bound(ob: &Bound<'_, PyAny>) -> PyResult<Self> {
        ob.extract::<u8>()? // 1-based month
            .saturating_sub(1)
            .try_into()
            .or_else(|_| Err(PyValueError::new_err("invalid month")))
    }
}

impl<'py> IntoPyObject<'py> for Month {
    #[cfg(Py_LIMITED_API)]
    type Target = PyInt;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyInt;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        (self as u8)
            .into_pyobject(py)
            .or_else(|_| Err(PyValueError::new_err("invalid month")))

        // .map(|month| PyInt::::new(py, month.into()))
        // .ok_or_else(|| PyValueError::new_err("invalid month"))
    }
}

impl FromPyObject<'_> for Duration {
    fn extract_bound(ob: &Bound<'_, PyAny>) -> PyResult<Duration> {
        // Python size are much lower than rust size so we do not need bound checks.
        // 0 <= microseconds < 1000000
        // 0 <= seconds < 3600*24
        // -999999999 <= days <= 999999999
        #[cfg(not(Py_LIMITED_API))]
        let (days, seconds, microseconds) = {
            let delta = ob.downcast::<PyDelta>()?;
            (
                delta.get_days().into(),
                delta.get_seconds().into(),
                delta.get_microseconds().into(),
            )
        };
        #[cfg(Py_LIMITED_API)]
        let (days, seconds, microseconds) = {
            check_type(ob, &DatetimeTypes::get(ob.py()).timedelta, "PyDelta")?;
            (
                ob.getattr(intern!(ob.py(), "days"))?.extract()?,
                ob.getattr(intern!(ob.py(), "seconds"))?.extract()?,
                ob.getattr(intern!(ob.py(), "microseconds"))?.extract()?,
            )
        };
        Ok(
            Duration::days(days)
                + Duration::seconds(seconds)
                + Duration::microseconds(microseconds),
        )
    }
}

#[allow(deprecated)]
impl ToPyObject for Date {
    #[inline]
    fn to_object(&self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

#[allow(deprecated)]
impl IntoPy<PyObject> for Date {
    #[inline]
    fn into_py(self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

impl<'py> IntoPyObject<'py> for Date {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyDate;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        let DateArgs { year, month, day } = (&self).into();
        #[cfg(not(Py_LIMITED_API))]
        {
            PyDate::new(py, year, month, day)
        }

        #[cfg(Py_LIMITED_API)]
        {
            DatetimeTypes::try_get(py).and_then(|dt| dt.date.bind(py).call1((year, month, day)))
        }
    }
}

impl<'py> IntoPyObject<'py> for &Date {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyDate;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    #[inline]
    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        (*self).into_pyobject(py)
    }
}

impl FromPyObject<'_> for Date {
    fn extract_bound(ob: &Bound<'_, PyAny>) -> PyResult<Date> {
        #[cfg(not(Py_LIMITED_API))]
        {
            let date = ob.downcast::<PyDate>()?;
            py_date_to_naive_date(date)
        }
        #[cfg(Py_LIMITED_API)]
        {
            check_type(ob, &DatetimeTypes::get(ob.py()).date, "PyDate")?;
            py_date_to_naive_date(ob)
        }
    }
}

#[allow(deprecated)]
impl ToPyObject for Time {
    #[inline]
    fn to_object(&self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

#[allow(deprecated)]
impl IntoPy<PyObject> for Time {
    #[inline]
    fn into_py(self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

impl<'py> IntoPyObject<'py> for Time {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyTime;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        let TimeArgs {
            hour,
            min,
            sec,
            micro,
            truncated_leap_second,
        } = (&self).into();

        #[cfg(not(Py_LIMITED_API))]
        let time = PyTime::new(py, hour, min, sec, micro, None)?;

        #[cfg(Py_LIMITED_API)]
        let time = DatetimeTypes::try_get(py)
            .and_then(|dt| dt.time.bind(py).call1((hour, min, sec, micro)))?;

        if truncated_leap_second {
            warn_truncated_leap_second(&time);
        }

        Ok(time)
    }
}

impl<'py> IntoPyObject<'py> for &Time {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyTime;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    #[inline]
    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        (*self).into_pyobject(py)
    }
}

impl FromPyObject<'_> for Time {
    fn extract_bound(ob: &Bound<'_, PyAny>) -> PyResult<Time> {
        #[cfg(not(Py_LIMITED_API))]
        {
            let time = ob.downcast::<PyTime>()?;
            py_time_to_naive_time(time)
        }
        #[cfg(Py_LIMITED_API)]
        {
            check_type(ob, &DatetimeTypes::get(ob.py()).time, "PyTime")?;
            py_time_to_naive_time(ob)
        }
    }
}

#[allow(deprecated)]
impl ToPyObject for PrimitiveDateTime {
    #[inline]
    fn to_object(&self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

#[allow(deprecated)]
impl IntoPy<PyObject> for PrimitiveDateTime {
    #[inline]
    fn into_py(self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

impl<'py> IntoPyObject<'py> for PrimitiveDateTime {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyDateTime;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        let DateArgs { year, month, day } = (&self.date()).into();
        let TimeArgs {
            hour,
            min,
            sec,
            micro,
            truncated_leap_second,
        } = (&self.time()).into();

        #[cfg(not(Py_LIMITED_API))]
        let datetime = PyDateTime::new(py, year, month, day, hour, min, sec, micro, None)?;

        #[cfg(Py_LIMITED_API)]
        let datetime = DatetimeTypes::try_get(py).and_then(|dt| {
            dt.datetime
                .bind(py)
                .call1((year, month, day, hour, min, sec, micro))
        })?;

        if truncated_leap_second {
            warn_truncated_leap_second(&datetime);
        }

        Ok(datetime)
    }
}

impl<'py> IntoPyObject<'py> for &PrimitiveDateTime {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyDateTime;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    #[inline]
    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        (*self).into_pyobject(py)
    }
}

impl FromPyObject<'_> for PrimitiveDateTime {
    fn extract_bound(dt: &Bound<'_, PyAny>) -> PyResult<PrimitiveDateTime> {
        #[cfg(not(Py_LIMITED_API))]
        let dt = dt.downcast::<PyDateTime>()?;
        #[cfg(Py_LIMITED_API)]
        check_type(dt, &DatetimeTypes::get(dt.py()).datetime, "PyDateTime")?;

        // If the user tries to convert a timezone aware datetime into a naive one,
        // we return a hard error. We could silently remove tzinfo, or assume local timezone
        // and do a conversion, but better leave this decision to the user of the library.
        #[cfg(not(Py_LIMITED_API))]
        let has_tzinfo = dt.get_tzinfo().is_some();
        #[cfg(Py_LIMITED_API)]
        let has_tzinfo = !dt.getattr(intern!(dt.py(), "tzinfo"))?.is_none();
        if has_tzinfo {
            return Err(PyTypeError::new_err("expected a datetime without tzinfo"));
        }

        let dt = PrimitiveDateTime::new(py_date_to_naive_date(dt)?, py_time_to_naive_time(dt)?);
        Ok(dt)
    }
}

#[allow(deprecated)]
impl ToPyObject for OffsetDateTime {
    fn to_object(&self, py: Python<'_>) -> PyObject {
        // FIXME: convert to better timezone representation here than just convert to fixed offset
        // See https://github.com/PyO3/pyo3/issues/3266
        // let tz = self..offset().to_object(py).bind(py).downcast().unwrap();
        primitive_datetime_to_py_datetime(
            py,
            &PrimitiveDateTime::new(self.date(), self.time()),
            None, //Some(self.to_object(py).bind(py)),
        )
    }
}

#[allow(deprecated)]
impl IntoPy<PyObject> for OffsetDateTime {
    fn into_py(self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

impl<'py> IntoPyObject<'py> for OffsetDateTime {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyDateTime;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    #[inline]
    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        (&self).into_pyobject(py)
    }
}

impl<'py> IntoPyObject<'py> for &OffsetDateTime {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyDateTime;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        let tz = self.offset().into_pyobject(py)?;
        let DateArgs { year, month, day } = (&self.date()).into();
        let TimeArgs {
            hour,
            min,
            sec,
            micro,
            truncated_leap_second,
        } = (&self.time()).into();

        #[cfg(not(Py_LIMITED_API))]
        let datetime = PyDateTime::new(py, year, month, day, hour, min, sec, micro, Some(&tz))?;

        #[cfg(Py_LIMITED_API)]
        let datetime = DatetimeTypes::try_get(py).and_then(|dt| {
            dt.datetime
                .bind(py)
                .call1((year, month, day, hour, min, sec, micro, tz))
        })?;

        if truncated_leap_second {
            warn_truncated_leap_second(&datetime);
        }

        Ok(datetime)
    }
}

impl FromPyObject<'_> for OffsetDateTime {
    fn extract_bound(dt: &Bound<'_, PyAny>) -> PyResult<OffsetDateTime> {
        #[cfg(not(Py_LIMITED_API))]
        let dt = dt.downcast::<PyDateTime>()?;
        #[cfg(Py_LIMITED_API)]
        check_type(dt, &DatetimeTypes::get(dt.py()).datetime, "PyDateTime")?;

        #[cfg(not(Py_LIMITED_API))]
        let tzinfo = dt.get_tzinfo();
        #[cfg(Py_LIMITED_API)]
        let tzinfo: Option<Bound<'_, PyAny>> = dt.getattr(intern!(dt.py(), "tzinfo"))?.extract()?;

        let tz = if let Some(tzinfo) = tzinfo {
            tzinfo.extract()?
        } else {
            return Err(PyTypeError::new_err(
                "expected a datetime with non-None tzinfo",
            ));
        };
        let naive_dt =
            PrimitiveDateTime::new(py_date_to_naive_date(dt)?, py_time_to_naive_time(dt)?);
        Ok(naive_dt.assume_offset(tz))
        // .ok_or_else(|| {
        //     PyValueError::new_err(format!(
        //         "The datetime {:?} contains an incompatible or ambiguous timezone",
        //         dt
        //     ))
        // })
    }
}

#[allow(deprecated)]
impl ToPyObject for UtcOffset {
    #[inline]
    fn to_object(&self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

#[allow(deprecated)]
impl IntoPy<PyObject> for UtcOffset {
    #[inline]
    fn into_py(self, py: Python<'_>) -> PyObject {
        self.into_pyobject(py).unwrap().into_any().unbind()
    }
}

impl<'py> IntoPyObject<'py> for UtcOffset {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyTzInfo;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        let seconds_offset = self.whole_seconds();
        #[cfg(not(Py_LIMITED_API))]
        {
            let td = PyDelta::new(py, 0, seconds_offset, 0, true)?;
            timezone_from_offset(&td)
        }

        #[cfg(Py_LIMITED_API)]
        {
            let td = Duration::seconds(seconds_offset.into()).into_pyobject(py)?;
            DatetimeTypes::try_get(py).and_then(|dt| dt.timezone.bind(py).call1((td,)))
        }
    }
}

impl<'py> IntoPyObject<'py> for &UtcOffset {
    #[cfg(Py_LIMITED_API)]
    type Target = PyAny;
    #[cfg(not(Py_LIMITED_API))]
    type Target = PyTzInfo;
    type Output = Bound<'py, Self::Target>;
    type Error = PyErr;

    #[inline]
    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        (*self).into_pyobject(py)
    }
}

impl FromPyObject<'_> for UtcOffset {
    /// Convert python tzinfo to rust [`FixedOffset`].
    ///
    /// Note that the conversion will result in precision lost in microseconds as chrono offset
    /// does not supports microseconds.
    fn extract_bound(ob: &Bound<'_, PyAny>) -> PyResult<UtcOffset> {
        #[cfg(not(Py_LIMITED_API))]
        let ob = ob.downcast::<PyTzInfo>()?;
        #[cfg(Py_LIMITED_API)]
        check_type(ob, &DatetimeTypes::get(ob.py()).tzinfo, "PyTzInfo")?;

        // Passing Python's None to the `utcoffset` function will only
        // work for timezones defined as fixed offsets in Python.
        // Any other timezone would require a datetime as the parameter, and return
        // None if the datetime is not provided.
        // Trying to convert None to a PyDelta in the next line will then fail.
        let py_timedelta = ob.call_method1("utcoffset", (PyNone::get(ob.py()),))?;
        if py_timedelta.is_none() {
            return Err(PyTypeError::new_err(format!(
                "{:?} is not a fixed offset timezone",
                ob
            )));
        }
        let total_seconds: Duration = py_timedelta.extract()?;
        // This cast is safe since the timedelta is limited to -24 hours and 24 hours.
        let total_seconds = total_seconds.whole_seconds() as i32;
        UtcOffset::from_whole_seconds(total_seconds)
            .or_else(|_| Err(PyValueError::new_err("fixed offset out of bounds")))
        // .ok_or_else(|| PyValueError::new_err("fixed offset out of bounds"))
    }
}

// #[allow(deprecated)]
// impl ToPyObject for Utc {
//     #[inline]
//     fn to_object(&self, py: Python<'_>) -> PyObject {
//         self.into_pyobject(py).unwrap().into_any().unbind()
//     }
// }

// #[allow(deprecated)]
// impl IntoPy<PyObject> for Utc {
//     #[inline]
//     fn into_py(self, py: Python<'_>) -> PyObject {
//         self.into_pyobject(py).unwrap().into_any().unbind()
//     }
// }

// impl<'py> IntoPyObject<'py> for Utc {
//     #[cfg(Py_LIMITED_API)]
//     type Target = PyAny;
//     #[cfg(not(Py_LIMITED_API))]
//     type Target = PyTzInfo;
//     type Output = Bound<'py, Self::Target>;
//     type Error = PyErr;

//     fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
//         #[cfg(Py_LIMITED_API)]
//         {
//             Ok(timezone_utc(py).into_any())
//         }
//         #[cfg(not(Py_LIMITED_API))]
//         {
//             Ok(timezone_utc(py))
//         }
//     }
// }

// impl<'py> IntoPyObject<'py> for &Utc {
//     #[cfg(Py_LIMITED_API)]
//     type Target = PyAny;
//     #[cfg(not(Py_LIMITED_API))]
//     type Target = PyTzInfo;
//     type Output = Bound<'py, Self::Target>;
//     type Error = PyErr;

//     #[inline]
//     fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
//         (*self).into_pyobject(py)
//     }
// }

// impl FromPyObject<'_> for Utc {
//     fn extract_bound(ob: &Bound<'_, PyAny>) -> PyResult<Utc> {
//         let py_utc = timezone_utc(ob.py());
//         if ob.eq(py_utc)? {
//             Ok(Utc)
//         } else {
//             Err(PyValueError::new_err("expected datetime.timezone.utc"))
//         }
//     }
// }

struct DateArgs {
    year: i32,
    month: u8,
    day: u8,
}

impl From<&Date> for DateArgs {
    fn from(value: &Date) -> Self {
        Self {
            year: value.year(),
            month: value.month() as u8,
            day: value.day() as u8,
        }
    }
}

struct TimeArgs {
    hour: u8,
    min: u8,
    sec: u8,
    micro: u32,
    truncated_leap_second: bool,
}

impl From<&Time> for TimeArgs {
    fn from(value: &Time) -> Self {
        let ns = value.nanosecond();
        let checked_sub = ns.checked_sub(1_000_000_000);
        let truncated_leap_second = checked_sub.is_some();
        let micro = checked_sub.unwrap_or(ns) / 1000;
        Self {
            hour: value.hour() as u8,
            min: value.minute() as u8,
            sec: value.second() as u8,
            micro,
            truncated_leap_second,
        }
    }
}

fn primitive_datetime_to_py_datetime(
    py: Python<'_>,
    primitive_date_time: &PrimitiveDateTime,
    #[cfg(not(Py_LIMITED_API))] tzinfo: Option<&Bound<'_, PyTzInfo>>,
    #[cfg(Py_LIMITED_API)] tzinfo: Option<&Bound<'_, PyAny>>,
) -> PyObject {
    let DateArgs { year, month, day } = (&primitive_date_time.date()).into();
    let TimeArgs {
        hour,
        min,
        sec,
        micro,
        truncated_leap_second,
    } = (&primitive_date_time.time()).into();
    #[cfg(not(Py_LIMITED_API))]
    let datetime = PyDateTime::new(py, year, month, day, hour, min, sec, micro, tzinfo)
        .expect("failed to construct datetime");
    #[cfg(Py_LIMITED_API)]
    let datetime = DatetimeTypes::get(py)
        .datetime
        .bind(py)
        .call1((year, month, day, hour, min, sec, micro, tzinfo))
        .expect("failed to construct datetime.datetime");
    if truncated_leap_second {
        warn_truncated_leap_second(&datetime);
    }
    datetime.into()
}

fn warn_truncated_leap_second(obj: &Bound<'_, PyAny>) {
    let py = obj.py();
    if let Err(e) = PyErr::warn(
        py,
        &py.get_type::<PyUserWarning>(),
        ffi::c_str!("ignored leap-second, `datetime` does not support leap-seconds"),
        0,
    ) {
        e.write_unraisable(py, Some(obj))
    };
}

#[cfg(not(Py_LIMITED_API))]
fn py_date_to_naive_date(py_date: &impl PyDateAccess) -> PyResult<Date> {
    Date::from_calendar_date(
        py_date.get_year(),
        py_date
            .get_month()
            .try_into()
            .or_else(|_| Err(PyValueError::new_err("invalid month")))?,
        py_date.get_day().into(),
    )
    .or_else(|_| Err(PyValueError::new_err("invalid or out-of-range date")))
    // .ok_or_else(|| PyValueError::new_err("invalid or out-of-range date"))
}

#[cfg(Py_LIMITED_API)]
fn py_date_to_naive_date(py_date: &Bound<'_, PyAny>) -> PyResult<Date> {
    Date::from_calendar_date(
        py_date.getattr(intern!(py_date.py(), "year"))?.extract()?,
        py_date.getattr(intern!(py_date.py(), "month"))?.extract()?,
        py_date.getattr(intern!(py_date.py(), "day"))?.extract()?,
    )
    .or_else(|_| Err(PyValueError::new_err("invalid or out-of-range date")))
}

#[cfg(not(Py_LIMITED_API))]
fn py_time_to_naive_time(py_time: &impl PyTimeAccess) -> PyResult<Time> {
    Time::from_hms_micro(
        py_time.get_hour().into(),
        py_time.get_minute().into(),
        py_time.get_second().into(),
        py_time.get_microsecond(),
    )
    .or_else(|_| Err(PyValueError::new_err("invalid or out-of-range time")))
    // .ok_or_else(|| PyValueError::new_err("invalid or out-of-range time"))
}

#[cfg(Py_LIMITED_API)]
fn py_time_to_naive_time(py_time: &Bound<'_, PyAny>) -> PyResult<Time> {
    Time::from_hms_micro(
        py_time.getattr(intern!(py_time.py(), "hour"))?.extract()?,
        py_time
            .getattr(intern!(py_time.py(), "minute"))?
            .extract()?,
        py_time
            .getattr(intern!(py_time.py(), "second"))?
            .extract()?,
        py_time
            .getattr(intern!(py_time.py(), "microsecond"))?
            .extract()?,
    )
    .or_else(|_| Err(PyValueError::new_err("invalid or out-of-range time")))
    // .ok_or_else(|| PyValueError::new_err("invalid or out-of-range time"))
}

#[cfg(Py_LIMITED_API)]
fn check_type(value: &Bound<'_, PyAny>, t: &PyObject, type_name: &'static str) -> PyResult<()> {
    if !value.is_instance(t.bind(value.py()))? {
        return Err(DowncastError::new(value, type_name).into());
    }
    Ok(())
}

#[cfg(Py_LIMITED_API)]
struct DatetimeTypes {
    date: PyObject,
    datetime: PyObject,
    time: PyObject,
    timedelta: PyObject,
    timezone: PyObject,
    timezone_utc: PyObject,
    tzinfo: PyObject,
}

#[cfg(Py_LIMITED_API)]
impl DatetimeTypes {
    fn get(py: Python<'_>) -> &Self {
        Self::try_get(py).expect("failed to load datetime module")
    }

    fn try_get(py: Python<'_>) -> PyResult<&Self> {
        static TYPES: GILOnceCell<DatetimeTypes> = GILOnceCell::new();
        TYPES.get_or_try_init(py, || {
            let datetime = py.import("datetime")?;
            let timezone = datetime.getattr("timezone")?;
            Ok::<_, PyErr>(Self {
                date: datetime.getattr("date")?.into(),
                datetime: datetime.getattr("datetime")?.into(),
                time: datetime.getattr("time")?.into(),
                timedelta: datetime.getattr("timedelta")?.into(),
                timezone_utc: timezone.getattr("utc")?.into(),
                timezone: timezone.into(),
                tzinfo: datetime.getattr("tzinfo")?.into(),
            })
        })
    }
}

#[cfg(Py_LIMITED_API)]
fn timezone_utc(py: Python<'_>) -> Bound<'_, PyAny> {
    DatetimeTypes::get(py).timezone_utc.bind(py).clone()
}

#[cfg(test)]
mod tests_time {
    use super::*;
    use crate::{types::PyTuple, BoundObject};
    use std::{cmp::Ordering, panic};

    #[test]
    // Only Python>=3.9 has the zoneinfo package
    // We skip the test on windows too since we'd need to install
    // tzdata there to make this work.
    #[cfg(all(Py_3_9, not(target_os = "windows")))]
    fn test_zoneinfo_is_not_fixed_offset() {
        use crate::ffi;
        use crate::types::any::PyAnyMethods;
        use crate::types::dict::PyDictMethods;

        Python::with_gil(|py| {
            let locals = crate::types::PyDict::new(py);
            py.run(
                ffi::c_str!("import zoneinfo; zi = zoneinfo.ZoneInfo('Europe/London')"),
                None,
                Some(&locals),
            )
            .unwrap();
            let result: PyResult<UtcOffset> = locals.get_item("zi").unwrap().unwrap().extract();
            assert!(result.is_err());
            let res = result.err().unwrap();
            // Also check the error message is what we expect
            let msg = res.value(py).repr().unwrap().to_string();
            assert_eq!(msg, "TypeError(\"zoneinfo.ZoneInfo(key='Europe/London') is not a fixed offset timezone\")");
        });
    }

    #[test]
    fn test_timezone_aware_to_naive_fails() {
        // Test that if a user tries to convert a python's timezone aware datetime into a naive
        // one, the conversion fails.
        Python::with_gil(|py| {
            let py_datetime =
                new_py_datetime_ob(py, "datetime", (2022, 1, 1, 1, 0, 0, 0, python_utc(py)));
            // Now test that converting a PyDateTime with tzinfo to a PrimitiveDateTime fails
            let res: PyResult<PrimitiveDateTime> = py_datetime.extract();
            assert_eq!(
                res.unwrap_err().value(py).repr().unwrap().to_string(),
                "TypeError('expected a datetime without tzinfo')"
            );
        });
    }

    #[test]
    fn test_naive_to_timezone_aware_fails() {
        // Test that if a user tries to convert a python's timezone aware datetime into a naive
        // one, the conversion fails.
        Python::with_gil(|py| {
            let py_datetime = new_py_datetime_ob(py, "datetime", (2022, 1, 1, 1, 0, 0, 0));
            // Now test that converting a PyDateTime with tzinfo to a PrimitiveDateTime fails
            let res: PyResult<OffsetDateTime> = py_datetime.extract();
            assert_eq!(
                res.unwrap_err().value(py).repr().unwrap().to_string(),
                "TypeError('expected a datetime with non-None tzinfo')"
            );

            // Now test that converting a PyDateTime with tzinfo to a PrimitiveDateTime fails
            let res: PyResult<OffsetDateTime> = py_datetime.extract();
            assert_eq!(
                res.unwrap_err().value(py).repr().unwrap().to_string(),
                "TypeError('expected a datetime with non-None tzinfo')"
            );
        });
    }

    #[test]
    fn test_invalid_types_fail() {
        // Test that if a user tries to convert a python's timezone aware datetime into a naive
        // one, the conversion fails.
        Python::with_gil(|py| {
            let none = py.None().into_bound(py);
            assert_eq!(
                none.extract::<Duration>().unwrap_err().to_string(),
                "TypeError: 'NoneType' object cannot be converted to 'PyDelta'"
            );
            assert_eq!(
                none.extract::<UtcOffset>().unwrap_err().to_string(),
                "TypeError: 'NoneType' object cannot be converted to 'PyTzInfo'"
            );
            // assert_eq!(
            //     none.extract::<Utc>().unwrap_err().to_string(),
            //     "ValueError: expected datetime.timezone.utc"
            // );
            assert_eq!(
                none.extract::<Time>().unwrap_err().to_string(),
                "TypeError: 'NoneType' object cannot be converted to 'PyTime'"
            );
            assert_eq!(
                none.extract::<Date>().unwrap_err().to_string(),
                "TypeError: 'NoneType' object cannot be converted to 'PyDate'"
            );
            assert_eq!(
                none.extract::<PrimitiveDateTime>().unwrap_err().to_string(),
                "TypeError: 'NoneType' object cannot be converted to 'PyDateTime'"
            );
            assert_eq!(
                none.extract::<OffsetDateTime>().unwrap_err().to_string(),
                "TypeError: 'NoneType' object cannot be converted to 'PyDateTime'"
            );
            assert_eq!(
                none.extract::<OffsetDateTime>().unwrap_err().to_string(),
                "TypeError: 'NoneType' object cannot be converted to 'PyDateTime'"
            );
        });
    }

    #[test]
    fn test_pyo3_timedelta_into_pyobject() {
        // Utility function used to check different durations.
        // The `name` parameter is used to identify the check in case of a failure.
        let check = |name: &'static str, delta: Duration, py_days, py_seconds, py_ms| {
            Python::with_gil(|py| {
                let delta = delta.into_pyobject(py).unwrap();
                let py_delta = new_py_datetime_ob(py, "timedelta", (py_days, py_seconds, py_ms));
                assert!(
                    delta.eq(&py_delta).unwrap(),
                    "{}: {} != {}",
                    name,
                    delta,
                    py_delta
                );
            });
        };

        let delta = Duration::days(-1) + Duration::seconds(1) + Duration::microseconds(-10);
        check("delta normalization", delta, -1, 1, -10);

        // Check the minimum value allowed by PyDelta, which is different
        // from the minimum value allowed in Duration. This should pass.
        let delta = Duration::seconds(-86399999913600); // min
        check("delta min value", delta, -999999999, 0, 0);

        // Same, for max value
        let delta = Duration::seconds(86399999999999) + Duration::nanoseconds(999999000); // max
        check("delta max value", delta, 999999999, 86399, 999999);

        // Also check that trying to convert an out of bound value errors.
        Python::with_gil(|py| {
            assert!(Duration::MIN.into_pyobject(py).is_err());
            assert!(Duration::MAX.into_pyobject(py).is_err());
        });
    }

    #[test]
    fn test_pyo3_timedelta_frompyobject() {
        // Utility function used to check different Durations.
        // The `name` parameter is used to identify the check in case of a failure.
        let check = |name: &'static str, delta: Duration, py_days, py_seconds, py_ms| {
            Python::with_gil(|py| {
                let py_delta = new_py_datetime_ob(py, "timedelta", (py_days, py_seconds, py_ms));
                let py_delta: Duration = py_delta.extract().unwrap();
                assert_eq!(py_delta, delta, "{}: {} != {}", name, py_delta, delta);
            })
        };

        // Check the minimum value allowed by PyDelta, which is different
        // from the minimum value allowed in Duration. This should pass.
        check(
            "min py_delta value",
            Duration::seconds(-86399999913600),
            -999999999,
            0,
            0,
        );
        // Same, for max value
        check(
            "max py_delta value",
            Duration::seconds(86399999999999) + Duration::microseconds(999999),
            999999999,
            86399,
            999999,
        );

        // This check is to assert that we can't construct every possible Duration from a PyDelta
        // since they have different bounds.
        Python::with_gil(|py| {
            let low_days: i32 = -1000000000;
            // This is possible
            assert!(panic::catch_unwind(|| Duration::days(low_days as i64)).is_ok());
            // This panics on PyDelta::new
            assert!(panic::catch_unwind(|| {
                let py_delta = new_py_datetime_ob(py, "timedelta", (low_days, 0, 0));
                if let Ok(_duration) = py_delta.extract::<Duration>() {
                    // So we should never get here
                }
            })
            .is_err());

            let high_days: i32 = 1000000000;
            // This is possible
            assert!(panic::catch_unwind(|| Duration::days(high_days as i64)).is_ok());
            // This panics on PyDelta::new
            assert!(panic::catch_unwind(|| {
                let py_delta = new_py_datetime_ob(py, "timedelta", (high_days, 0, 0));
                if let Ok(_duration) = py_delta.extract::<Duration>() {
                    // So we should never get here
                }
            })
            .is_err());
        });
    }

    #[test]
    fn test_pyo3_date_into_pyobject() {
        let eq_ymd = |name: &'static str, year, month, day| {
            Python::with_gil(|py| {
                let month = Month::try_from(month).unwrap();
                let date = Date::from_calendar_date(year, month, day)
                    .unwrap()
                    .into_pyobject(py)
                    .unwrap();
                let py_date = new_py_datetime_ob(py, "date", (year, month, day));
                assert_eq!(
                    date.compare(&py_date).unwrap(),
                    Ordering::Equal,
                    "{}: {} != {}",
                    name,
                    date,
                    py_date
                );
            })
        };

        eq_ymd("past date", 2012, 2, 29);
        eq_ymd("min date", 1, 1, 1);
        eq_ymd("future date", 3000, 6, 5);
        eq_ymd("max date", 9999, 12, 31);
    }

    #[test]
    fn test_pyo3_date_frompyobject() {
        let eq_ymd = |name: &'static str, year, month, day| {
            let month = Month::try_from(month).unwrap();
            Python::with_gil(|py| {
                let py_date = new_py_datetime_ob(py, "date", (year, month, day));
                let py_date: Date = py_date.extract().unwrap();
                let date = Date::from_calendar_date(year, month, day).unwrap();
                assert_eq!(py_date, date, "{}: {} != {}", name, date, py_date);
            })
        };

        eq_ymd("past date", 2012, 2, 29);
        eq_ymd("min date", 1, 1, 1);
        eq_ymd("future date", 3000, 6, 5);
        eq_ymd("max date", 9999, 12, 31);
    }

    // #[test]
    // fn test_pyo3_datetime_into_pyobject_utc() {
    //     Python::with_gil(|py| {
    //         let check_utc =
    //             |name: &'static str, year, month, day, hour, minute, second, ms, py_ms| {
    //                 let month = Month::try_from(month).unwrap();
    //                 let datetime = Date::from_calendar_date(year, month, day)
    //                     .unwrap()
    //                     .with_hms_micro(hour, minute, second, ms)
    //                     .unwrap()
    //                     .assume_utc();
    //                 let datetime = datetime.into_pyobject(py).unwrap();
    //                 let py_datetime = new_py_datetime_ob(
    //                     py,
    //                     "datetime",
    //                     (
    //                         year,
    //                         month as u8,
    //                         day,
    //                         hour,
    //                         minute,
    //                         second,
    //                         py_ms,
    //                         python_utc(py),
    //                     ),
    //                 );
    //                 assert_eq!(
    //                     datetime.compare(&py_datetime).unwrap(),
    //                     Ordering::Equal,
    //                     "{}: {} != {}",
    //                     name,
    //                     datetime,
    //                     py_datetime
    //                 );
    //             };

    //         check_utc("regular", 2014, 5, 6, 7, 8, 9, 999_999, 999_999);

    //         #[cfg(not(Py_GIL_DISABLED))]
    //         assert_warnings!(
    //             py,
    //             check_utc("leap second", 2014, 5, 6, 7, 8, 59, 1_999_999, 999_999),
    //             [(
    //                 PyUserWarning,
    //                 "ignored leap-second, `datetime` does not support leap-seconds"
    //             )]
    //         );
    //     })
    // }

    // #[test]
    // fn test_pyo3_datetime_into_pyobject_fixed_offset() {
    //     Python::with_gil(|py| {
    //         let check_fixed_offset =
    //             |name: &'static str, year, month, day, hour, minute, second, ms, py_ms| {
    //                 let offset = FixedOffset::east_opt(3600).unwrap();
    //                 let datetime = Date::from_ymd_opt(year, month, day)
    //                     .unwrap()
    //                     .and_hms_micro_opt(hour, minute, second, ms)
    //                     .unwrap()
    //                     .and_local_timezone(offset)
    //                     .unwrap();
    //                 let datetime = datetime.into_pyobject(py).unwrap();
    //                 let py_tz = offset.into_pyobject(py).unwrap();
    //                 let py_datetime = new_py_datetime_ob(
    //                     py,
    //                     "datetime",
    //                     (year, month, day, hour, minute, second, py_ms, py_tz),
    //                 );
    //                 assert_eq!(
    //                     datetime.compare(&py_datetime).unwrap(),
    //                     Ordering::Equal,
    //                     "{}: {} != {}",
    //                     name,
    //                     datetime,
    //                     py_datetime
    //                 );
    //             };

    //         check_fixed_offset("regular", 2014, 5, 6, 7, 8, 9, 999_999, 999_999);

    //         #[cfg(not(Py_GIL_DISABLED))]
    //         assert_warnings!(
    //             py,
    //             check_fixed_offset("leap second", 2014, 5, 6, 7, 8, 59, 1_999_999, 999_999),
    //             [(
    //                 PyUserWarning,
    //                 "ignored leap-second, `datetime` does not support leap-seconds"
    //             )]
    //         );
    //     })
    // }

    // #[test]
    // fn test_pyo3_datetime_frompyobject_utc() {
    //     Python::with_gil(|py| {
    //         let year = 2014;
    //         let month = 5;
    //         let day = 6;
    //         let hour = 7;
    //         let minute = 8;
    //         let second = 9;
    //         let micro = 999_999;
    //         let tz_utc = timezone_utc(py);
    //         let py_datetime = new_py_datetime_ob(
    //             py,
    //             "datetime",
    //             (year, month, day, hour, minute, second, micro, tz_utc),
    //         );
    //         let py_datetime: OffsetDateTime<Utc> = py_datetime.extract().unwrap();
    //         let datetime = Date::from_ymd_opt(year, month, day)
    //             .unwrap()
    //             .and_hms_micro_opt(hour, minute, second, micro)
    //             .unwrap()
    //             .and_utc();
    //         assert_eq!(py_datetime, datetime,);
    //     })
    // }

    // #[test]
    // fn test_pyo3_datetime_frompyobject_fixed_offset() {
    //     Python::with_gil(|py| {
    //         let year = 2014;
    //         let month = 5;
    //         let day = 6;
    //         let hour = 7;
    //         let minute = 8;
    //         let second = 9;
    //         let micro = 999_999;
    //         let offset = FixedOffset::east_opt(3600).unwrap();
    //         let py_tz = offset.into_pyobject(py).unwrap();
    //         let py_datetime = new_py_datetime_ob(
    //             py,
    //             "datetime",
    //             (year, month, day, hour, minute, second, micro, py_tz),
    //         );
    //         let datetime_from_py: OffsetDateTime<FixedOffset> = py_datetime.extract().unwrap();
    //         let datetime = Date::from_ymd_opt(year, month, day)
    //             .unwrap()
    //             .and_hms_micro_opt(hour, minute, second, micro)
    //             .unwrap();
    //         let datetime = datetime.and_local_timezone(offset).unwrap();

    //         assert_eq!(datetime_from_py, datetime);
    //         assert!(
    //             py_datetime.extract::<OffsetDateTime<Utc>>().is_err(),
    //             "Extracting Utc from nonzero FixedOffset timezone will fail"
    //         );

    //         let utc = python_utc(py);
    //         let py_datetime_utc = new_py_datetime_ob(
    //             py,
    //             "datetime",
    //             (year, month, day, hour, minute, second, micro, utc),
    //         );
    //         assert!(
    //             py_datetime_utc
    //                 .extract::<OffsetDateTime<FixedOffset>>()
    //                 .is_ok(),
    //             "Extracting FixedOffset from Utc timezone will succeed"
    //         );
    //     })
    // }

    // #[test]
    // fn test_pyo3_offset_fixed_into_pyobject() {
    //     Python::with_gil(|py| {
    //         // Chrono offset
    //         let offset = FixedOffset::east_opt(3600)
    //             .unwrap()
    //             .into_pyobject(py)
    //             .unwrap();
    //         // Python timezone from timedelta
    //         let td = new_py_datetime_ob(py, "timedelta", (0, 3600, 0));
    //         let py_timedelta = new_py_datetime_ob(py, "timezone", (td,));
    //         // Should be equal
    //         assert!(offset.eq(py_timedelta).unwrap());

    //         // Same but with negative values
    //         let offset = FixedOffset::east_opt(-3600)
    //             .unwrap()
    //             .into_pyobject(py)
    //             .unwrap();
    //         let td = new_py_datetime_ob(py, "timedelta", (0, -3600, 0));
    //         let py_timedelta = new_py_datetime_ob(py, "timezone", (td,));
    //         assert!(offset.eq(py_timedelta).unwrap());
    //     })
    // }

    // #[test]
    // fn test_pyo3_offset_fixed_frompyobject() {
    //     Python::with_gil(|py| {
    //         let py_timedelta = new_py_datetime_ob(py, "timedelta", (0, 3600, 0));
    //         let py_tzinfo = new_py_datetime_ob(py, "timezone", (py_timedelta,));
    //         let offset: FixedOffset = py_tzinfo.extract().unwrap();
    //         assert_eq!(FixedOffset::east_opt(3600).unwrap(), offset);
    //     })
    // }

    // #[test]
    // fn test_pyo3_offset_utc_into_pyobject() {
    //     Python::with_gil(|py| {
    //         let utc = Utc.into_pyobject(py).unwrap();
    //         let py_utc = python_utc(py);
    //         assert!(utc.is(&py_utc));
    //     })
    // }

    // #[test]
    // fn test_pyo3_offset_utc_frompyobject() {
    //     Python::with_gil(|py| {
    //         let py_utc = python_utc(py);
    //         let py_utc: Utc = py_utc.extract().unwrap();
    //         assert_eq!(Utc, py_utc);

    //         let py_timedelta = new_py_datetime_ob(py, "timedelta", (0, 0, 0));
    //         let py_timezone_utc = new_py_datetime_ob(py, "timezone", (py_timedelta,));
    //         let py_timezone_utc: Utc = py_timezone_utc.extract().unwrap();
    //         assert_eq!(Utc, py_timezone_utc);

    //         let py_timedelta = new_py_datetime_ob(py, "timedelta", (0, 3600, 0));
    //         let py_timezone = new_py_datetime_ob(py, "timezone", (py_timedelta,));
    //         assert!(py_timezone.extract::<Utc>().is_err());
    //     })
    // }

    // #[test]
    // fn test_pyo3_time_into_pyobject() {
    //     Python::with_gil(|py| {
    //         let check_time = |name: &'static str, hour, minute, second, ms, py_ms| {
    //             let time = Time::from_hms_micro_opt(hour, minute, second, ms)
    //                 .unwrap()
    //                 .into_pyobject(py)
    //                 .unwrap();
    //             let py_time = new_py_datetime_ob(py, "time", (hour, minute, second, py_ms));
    //             assert!(
    //                 time.eq(&py_time).unwrap(),
    //                 "{}: {} != {}",
    //                 name,
    //                 time,
    //                 py_time
    //             );
    //         };

    //         check_time("regular", 3, 5, 7, 999_999, 999_999);

    //         #[cfg(not(Py_GIL_DISABLED))]
    //         assert_warnings!(
    //             py,
    //             check_time("leap second", 3, 5, 59, 1_999_999, 999_999),
    //             [(
    //                 PyUserWarning,
    //                 "ignored leap-second, `datetime` does not support leap-seconds"
    //             )]
    //         );
    //     })
    // }

    // #[test]
    // fn test_pyo3_time_frompyobject() {
    //     let hour = 3;
    //     let minute = 5;
    //     let second = 7;
    //     let micro = 999_999;
    //     Python::with_gil(|py| {
    //         let py_time = new_py_datetime_ob(py, "time", (hour, minute, second, micro));
    //         let py_time: Time = py_time.extract().unwrap();
    //         let time = Time::from_hms_micro_opt(hour, minute, second, micro).unwrap();
    //         assert_eq!(py_time, time);
    //     })
    // }

    fn new_py_datetime_ob<'py, A>(py: Python<'py>, name: &str, args: A) -> Bound<'py, PyAny>
    where
        A: IntoPyObject<'py, Target = PyTuple>,
    {
        py.import("datetime")
            .unwrap()
            .getattr(name)
            .unwrap()
            .call1(
                args.into_pyobject(py)
                    .map_err(Into::into)
                    .unwrap()
                    .into_bound(),
            )
            .unwrap()
    }

    fn python_utc(py: Python<'_>) -> Bound<'_, PyAny> {
        py.import("datetime")
            .unwrap()
            .getattr("timezone")
            .unwrap()
            .getattr("utc")
            .unwrap()
    }

    // #[cfg(not(any(target_arch = "wasm32", Py_GIL_DISABLED)))]
    // mod proptests {
    //     use super::*;
    //     use crate::tests::common::CatchWarnings;
    //     use crate::types::IntoPyDict;
    //     use proptest::prelude::*;
    //     use std::ffi::CString;

    //     proptest! {

    //         // Range is limited to 1970 to 2038 due to windows limitations
    //         #[test]
    //         fn test_pyo3_offset_fixed_frompyobject_created_in_python(timestamp in 0..(i32::MAX as i64), timedelta in -86399i32..=86399i32) {
    //             Python::with_gil(|py| {

    //                 let globals = [("datetime", py.import("datetime").unwrap())].into_py_dict(py).unwrap();
    //                 let code = format!("datetime.datetime.fromtimestamp({}).replace(tzinfo=datetime.timezone(datetime.timedelta(seconds={})))", timestamp, timedelta);
    //                 let t = py.eval(&CString::new(code).unwrap(), Some(&globals), None).unwrap();

    //                 // Get ISO 8601 string from python
    //                 let py_iso_str = t.call_method0("isoformat").unwrap();

    //                 // Get ISO 8601 string from rust
    //                 let t = t.extract::<OffsetDateTime<FixedOffset>>().unwrap();
    //                 // Python doesn't print the seconds of the offset if they are 0
    //                 let rust_iso_str = if timedelta % 60 == 0 {
    //                     t.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
    //                 } else {
    //                     t.format("%Y-%m-%dT%H:%M:%S%::z").to_string()
    //                 };

    //                 // They should be equal
    //                 assert_eq!(py_iso_str.to_string(), rust_iso_str);
    //             })
    //         }

    //         #[test]
    //         fn test_duration_roundtrip(days in -999999999i64..=999999999i64) {
    //             // Test roundtrip conversion rust->python->rust for all allowed
    //             // python values of durations (from -999999999 to 999999999 days),
    //             Python::with_gil(|py| {
    //                 let dur = Duration::days(days);
    //                 let py_delta = dur.into_pyobject(py).unwrap();
    //                 let roundtripped: Duration = py_delta.extract().expect("Round trip");
    //                 assert_eq!(dur, roundtripped);
    //             })
    //         }

    //         #[test]
    //         fn test_fixed_offset_roundtrip(secs in -86399i32..=86399i32) {
    //             Python::with_gil(|py| {
    //                 let offset = FixedOffset::east_opt(secs).unwrap();
    //                 let py_offset = offset.into_pyobject(py).unwrap();
    //                 let roundtripped: FixedOffset = py_offset.extract().expect("Round trip");
    //                 assert_eq!(offset, roundtripped);
    //             })
    //         }

    //         #[test]
    //         fn test_naive_date_roundtrip(
    //             year in 1i32..=9999i32,
    //             month in 1u32..=12u32,
    //             day in 1u32..=31u32
    //         ) {
    //             // Test roundtrip conversion rust->python->rust for all allowed
    //             // python dates (from year 1 to year 9999)
    //             Python::with_gil(|py| {
    //                 // We use to `from_ymd_opt` constructor so that we only test valid `Date`s.
    //                 // This is to skip the test if we are creating an invalid date, like February 31.
    //                 if let Some(date) = Date::from_ymd_opt(year, month, day) {
    //                     let py_date = date.into_pyobject(py).unwrap();
    //                     let roundtripped: Date = py_date.extract().expect("Round trip");
    //                     assert_eq!(date, roundtripped);
    //                 }
    //             })
    //         }

    //         #[test]
    //         fn test_naive_time_roundtrip(
    //             hour in 0u32..=23u32,
    //             min in 0u32..=59u32,
    //             sec in 0u32..=59u32,
    //             micro in 0u32..=1_999_999u32
    //         ) {
    //             // Test roundtrip conversion rust->python->rust for naive times.
    //             // Python time has a resolution of microseconds, so we only test
    //             // NaiveTimes with microseconds resolution, even if Time has nanosecond
    //             // resolution.
    //             Python::with_gil(|py| {
    //                 if let Some(time) = Time::from_hms_micro_opt(hour, min, sec, micro) {
    //                     // Wrap in CatchWarnings to avoid to_object firing warning for truncated leap second
    //                     let py_time = CatchWarnings::enter(py, |_| time.into_pyobject(py)).unwrap();
    //                     let roundtripped: Time = py_time.extract().expect("Round trip");
    //                     // Leap seconds are not roundtripped
    //                     let expected_roundtrip_time = micro.checked_sub(1_000_000).map(|micro| Time::from_hms_micro_opt(hour, min, sec, micro).unwrap()).unwrap_or(time);
    //                     assert_eq!(expected_roundtrip_time, roundtripped);
    //                 }
    //             })
    //         }

    //         #[test]
    //         fn test_naive_datetime_roundtrip(
    //             year in 1i32..=9999i32,
    //             month in 1u32..=12u32,
    //             day in 1u32..=31u32,
    //             hour in 0u32..=24u32,
    //             min in 0u32..=60u32,
    //             sec in 0u32..=60u32,
    //             micro in 0u32..=999_999u32
    //         ) {
    //             Python::with_gil(|py| {
    //                 let date_opt = Date::from_ymd_opt(year, month, day);
    //                 let time_opt = Time::from_hms_micro_opt(hour, min, sec, micro);
    //                 if let (Some(date), Some(time)) = (date_opt, time_opt) {
    //                     let dt = PrimitiveDateTime::new(date, time);
    //                     let pydt = dt.into_pyobject(py).unwrap();
    //                     let roundtripped: PrimitiveDateTime = pydt.extract().expect("Round trip");
    //                     assert_eq!(dt, roundtripped);
    //                 }
    //             })
    //         }

    //         #[test]
    //         fn test_utc_datetime_roundtrip(
    //             year in 1i32..=9999i32,
    //             month in 1u32..=12u32,
    //             day in 1u32..=31u32,
    //             hour in 0u32..=23u32,
    //             min in 0u32..=59u32,
    //             sec in 0u32..=59u32,
    //             micro in 0u32..=1_999_999u32
    //         ) {
    //             Python::with_gil(|py| {
    //                 let date_opt = Date::from_ymd_opt(year, month, day);
    //                 let time_opt = Time::from_hms_micro_opt(hour, min, sec, micro);
    //                 if let (Some(date), Some(time)) = (date_opt, time_opt) {
    //                     let dt: OffsetDateTime<Utc> = PrimitiveDateTime::new(date, time).and_utc();
    //                     // Wrap in CatchWarnings to avoid into_py firing warning for truncated leap second
    //                     let py_dt = CatchWarnings::enter(py, |_| dt.into_pyobject(py)).unwrap();
    //                     let roundtripped: OffsetDateTime<Utc> = py_dt.extract().expect("Round trip");
    //                     // Leap seconds are not roundtripped
    //                     let expected_roundtrip_time = micro.checked_sub(1_000_000).map(|micro| Time::from_hms_micro_opt(hour, min, sec, micro).unwrap()).unwrap_or(time);
    //                     let expected_roundtrip_dt: OffsetDateTime<Utc> = PrimitiveDateTime::new(date, expected_roundtrip_time).and_utc();
    //                     assert_eq!(expected_roundtrip_dt, roundtripped);
    //                 }
    //             })
    //         }

    //         #[test]
    //         fn test_fixed_offset_datetime_roundtrip(
    //             year in 1i32..=9999i32,
    //             month in 1u32..=12u32,
    //             day in 1u32..=31u32,
    //             hour in 0u32..=23u32,
    //             min in 0u32..=59u32,
    //             sec in 0u32..=59u32,
    //             micro in 0u32..=1_999_999u32,
    //             offset_secs in -86399i32..=86399i32
    //         ) {
    //             Python::with_gil(|py| {
    //                 let date_opt = Date::from_ymd_opt(year, month, day);
    //                 let time_opt = Time::from_hms_micro_opt(hour, min, sec, micro);
    //                 let offset = FixedOffset::east_opt(offset_secs).unwrap();
    //                 if let (Some(date), Some(time)) = (date_opt, time_opt) {
    //                     let dt: OffsetDateTime<FixedOffset> = PrimitiveDateTime::new(date, time).and_local_timezone(offset).unwrap();
    //                     // Wrap in CatchWarnings to avoid into_py firing warning for truncated leap second
    //                     let py_dt = CatchWarnings::enter(py, |_| dt.into_pyobject(py)).unwrap();
    //                     let roundtripped: OffsetDateTime<FixedOffset> = py_dt.extract().expect("Round trip");
    //                     // Leap seconds are not roundtripped
    //                     let expected_roundtrip_time = micro.checked_sub(1_000_000).map(|micro| Time::from_hms_micro_opt(hour, min, sec, micro).unwrap()).unwrap_or(time);
    //                     let expected_roundtrip_dt: OffsetDateTime<FixedOffset> = PrimitiveDateTime::new(date, expected_roundtrip_time).and_local_timezone(offset).unwrap();
    //                     assert_eq!(expected_roundtrip_dt, roundtripped);
    //                 }
    //             })
    //         }
    //     }
    // }
}