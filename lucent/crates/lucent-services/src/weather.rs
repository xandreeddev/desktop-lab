//! Open-Meteo transport, checked decoding and read-only legacy configuration.
//! Coordinates travel over stdin, never a command-line argument or error body.
use chrono::Datelike;
use lucent_domain::*;
use serde::Deserialize;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CONFIG_LIMIT: u64 = 4096;
const RESPONSE_LIMIT: usize = 64 * 1024;
#[derive(Default)]
pub struct OpenMeteo;
impl WeatherPort for OpenMeteo {
    fn legacy_preferences(&self) -> Result<Option<WeatherPreferences>> {
        read_legacy(&crate::config_directory().join("weather.json"))
    }
    fn forecast(
        &self,
        coordinates: WeatherCoordinates,
        stop: &dyn StopSignal,
    ) -> Result<WeatherSnapshot> {
        let body = fetch(coordinates, stop)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| bad_response())?
            .as_secs();
        parse(&body, now)
    }
}
fn bad_response() -> DomainError {
    DomainError::Failed("Weather provider returned invalid conditions or forecast".into())
}
fn unavailable() -> DomainError {
    DomainError::Unavailable(
        "Weather could not be refreshed; check the connection and retry".into(),
    )
}
fn read_legacy(path: &Path) -> Result<Option<WeatherPreferences>> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(DomainError::Invalid(
                "Legacy weather configuration could not be read safely".into(),
            ));
        }
    };
    let metadata = file.metadata().map_err(|_| {
        DomainError::Failed("Legacy weather configuration could not be inspected".into())
    })?;
    if !metadata.is_file() || metadata.len() > CONFIG_LIMIT {
        return Err(DomainError::Invalid(
            "Legacy weather configuration must be a small regular file".into(),
        ));
    }
    let mut bytes = vec![];
    file.take(CONFIG_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            DomainError::Failed("Legacy weather configuration could not be read".into())
        })?;
    if bytes.len() as u64 > CONFIG_LIMIT {
        return Err(DomainError::Invalid(
            "Legacy weather configuration is too large".into(),
        ));
    }
    #[derive(Deserialize)]
    struct Legacy {
        name: Option<String>,
        latitude: f64,
        longitude: f64,
    }
    let value: Legacy = serde_json::from_slice(&bytes).map_err(|_| {
        DomainError::Invalid(
            "Legacy weather configuration is invalid; correct it or apply a new location".into(),
        )
    })?;
    Ok(Some(WeatherPreferences {
        enabled: true,
        location: Some(WeatherLocation::new(
            value.name.unwrap_or_else(|| "Selected location".into()),
            WeatherCoordinates::new(value.latitude, value.longitude)?,
        )?),
        unit: TemperatureUnit::Celsius,
    }))
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn fetch(coordinates: WeatherCoordinates, stop: &dyn StopSignal) -> Result<Vec<u8>> {
    if stop.cancelled() {
        return Err(unavailable());
    }
    let mut child = OwnedChild(
        Command::new("curl")
            .args([
                "--config",
                "-",
                "--silent",
                "--fail",
                "--max-time",
                "4",
                "--connect-timeout",
                "2",
                "--proto",
                "=https",
                "--max-redirs",
                "0",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| unavailable())?,
    );
    let request = format!(
        "url = \"https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,weather_code,is_day&daily=weather_code,temperature_2m_max,temperature_2m_min&forecast_days=3&temperature_unit=celsius&timezone=auto&timeformat=unixtime\"\n",
        coordinates.latitude(),
        coordinates.longitude()
    );
    child
        .0
        .stdin
        .take()
        .ok_or_else(unavailable)?
        .write_all(request.as_bytes())
        .map_err(|_| unavailable())?;
    let stdout = child.0.stdout.take().ok_or_else(unavailable)?;
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("weather-response".into())
        .spawn(move || {
            let mut bytes = vec![];
            let result = stdout
                .take(RESPONSE_LIMIT as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = send.send(result);
        })
        .map_err(|_| unavailable())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = None;
    loop {
        if stop.cancelled() || Instant::now() >= deadline {
            return Err(unavailable());
        }
        if output.is_none() {
            match receive.try_recv() {
                Ok(value) => output = Some(value.map_err(|_| unavailable())?),
                Err(mpsc::TryRecvError::Disconnected) => return Err(unavailable()),
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if let Some(bytes) = &output
            && bytes.len() > RESPONSE_LIMIT
        {
            return Err(bad_response());
        }
        if let Some(status) = child.0.try_wait().map_err(|_| unavailable())? {
            if !status.success() {
                return Err(unavailable());
            }
            if let Some(bytes) = output {
                return Ok(bytes);
            }
        }
        stop.wait(Duration::from_millis(10));
    }
}
fn condition(code: u64) -> WeatherCondition {
    use WeatherCondition::*;
    match code {
        0 => Clear,
        1 => MostlyClear,
        2 => PartlyCloudy,
        3 => Overcast,
        45 | 48 => Fog,
        51 | 53 | 55 => Drizzle,
        56 | 57 | 66 | 67 => FreezingRain,
        61 | 63 | 65 => Rain,
        71 | 73 | 75 | 77 => Snow,
        80..=82 => Showers,
        85 | 86 => SnowShowers,
        95..=97 | 99 => Thunderstorm,
        _ => Unknown,
    }
}
fn parse(bytes: &[u8], fetched_at: u64) -> Result<WeatherSnapshot> {
    if bytes.len() > RESPONSE_LIMIT {
        return Err(bad_response());
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| bad_response())?;
    let units = &value["current_units"];
    let daily_units = &value["daily_units"];
    if units["temperature_2m"] != "°C"
        || units["time"] != "unixtime"
        || daily_units["time"] != "unixtime"
        || daily_units["temperature_2m_min"] != "°C"
        || daily_units["temperature_2m_max"] != "°C"
    {
        return Err(bad_response());
    }
    let current = &value["current"];
    let temperature = WeatherTemperature::new(
        current["temperature_2m"]
            .as_f64()
            .ok_or_else(bad_response)?,
    )
    .map_err(|_| bad_response())?;
    let observed_at = current["time"].as_u64().ok_or_else(bad_response)?;
    let is_day = match current["is_day"].as_u64() {
        Some(0) => false,
        Some(1) => true,
        _ => return Err(bad_response()),
    };
    let current_condition = condition(current["weather_code"].as_u64().ok_or_else(bad_response)?);
    let offset = value["utc_offset_seconds"]
        .as_i64()
        .filter(|value| (-86400..=86400).contains(value))
        .ok_or_else(bad_response)?;
    let daily = &value["daily"];
    let array = |name: &str| {
        daily[name]
            .as_array()
            .filter(|rows| rows.len() == WeatherSnapshot::DAYS)
            .ok_or_else(bad_response)
    };
    let times = array("time")?;
    let low = array("temperature_2m_min")?;
    let high = array("temperature_2m_max")?;
    let codes = array("weather_code")?;
    let mut days = Vec::with_capacity(WeatherSnapshot::DAYS);
    for index in 0..WeatherSnapshot::DAYS {
        let epoch = times[index]
            .as_i64()
            .and_then(|value| value.checked_add(offset))
            .ok_or_else(bad_response)?;
        let time = chrono::DateTime::from_timestamp(epoch, 0).ok_or_else(bad_response)?;
        let date = Date::new(time.year(), time.month(), time.day()).map_err(|_| bad_response())?;
        days.push(
            WeatherDay::new(
                date,
                condition(codes[index].as_u64().ok_or_else(bad_response)?),
                WeatherTemperature::new(low[index].as_f64().ok_or_else(bad_response)?)
                    .map_err(|_| bad_response())?,
                WeatherTemperature::new(high[index].as_f64().ok_or_else(bad_response)?)
                    .map_err(|_| bad_response())?,
            )
            .map_err(|_| bad_response())?,
        );
    }
    WeatherSnapshot::new(
        temperature,
        current_condition,
        is_day,
        observed_at,
        fetched_at,
        days,
    )
    .map_err(|_| bad_response())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    fn response() -> serde_json::Value {
        serde_json::json!({"utc_offset_seconds":0,"current_units":{"time":"unixtime","temperature_2m":"°C"},"daily_units":{"time":"unixtime","temperature_2m_min":"°C","temperature_2m_max":"°C"},"current":{"time":1768435200u64,"temperature_2m":7.,"weather_code":61,"is_day":0},"daily":{"time":[1768435200u64,1768521600u64,1768608000u64],"temperature_2m_min":[3.,2.,1.],"temperature_2m_max":[8.,9.,10.],"weather_code":[61,2,0]}})
    }
    #[test]
    fn current_and_three_day_forecast_are_checked_and_provider_neutral() {
        let snapshot = parse(&serde_json::to_vec(&response()).unwrap(), 1768436000).unwrap();
        assert_eq!(snapshot.condition, WeatherCondition::Rain);
        assert!(!snapshot.is_day);
        assert_eq!(snapshot.days()[1].condition, WeatherCondition::PartlyCloudy);
        assert_eq!(snapshot.days()[0].date, Date::new(2026, 1, 15).unwrap());
        for (field, replacement) in [
            ("temperature_2m", serde_json::json!(1e300)),
            ("is_day", serde_json::json!(2)),
            ("time", serde_json::json!(-1)),
        ] {
            let mut body = response();
            body["current"][field] = replacement;
            assert!(parse(&serde_json::to_vec(&body).unwrap(), 0).is_err());
        }
        let mut body = response();
        body["current"]["weather_code"] = serde_json::json!(4294967296u64);
        assert_eq!(
            parse(&serde_json::to_vec(&body).unwrap(), 0)
                .unwrap()
                .condition,
            WeatherCondition::Unknown
        );
    }
    #[test]
    fn malformed_units_ranges_arrays_and_dates_cannot_be_published() {
        for case in 0..5 {
            let mut body = response();
            match case {
                0 => body["current_units"]["temperature_2m"] = serde_json::json!("°F"),
                1 => body["daily"]["time"] = serde_json::json!([1, 2]),
                2 => body["daily"]["temperature_2m_min"][0] = serde_json::json!(20),
                3 => body["daily"]["time"][1] = body["daily"]["time"][0].clone(),
                _ => body["daily"]["weather_code"][0] = serde_json::Value::Null,
            };
            assert!(parse(&serde_json::to_vec(&body).unwrap(), 0).is_err());
        }
        assert!(parse(&vec![b' '; RESPONSE_LIMIT + 1], 0).is_err());
    }
    #[test]
    fn legacy_import_is_bounded_regular_and_preserves_original_bytes() {
        let directory = std::env::temp_dir().join(format!(
            "lucent-weather-config-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("weather.json");
        assert!(read_legacy(&path).unwrap().is_none());
        let original = br#"{"name":"Fixture place","latitude":0,"longitude":0}"#;
        std::fs::write(&path, original).unwrap();
        assert!(read_legacy(&path).unwrap().unwrap().enabled);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::write(&path, b"invalid").unwrap();
        assert!(read_legacy(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid");
        let link = directory.join("link");
        symlink(&path, &link).unwrap();
        assert!(read_legacy(&link).is_err());
        assert!(read_legacy(&directory).is_err());
        std::fs::write(&path, vec![b' '; CONFIG_LIMIT as usize + 1]).unwrap();
        assert!(read_legacy(&path).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
