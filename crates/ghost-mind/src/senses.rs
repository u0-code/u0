//! What she can perceive of the physical world she nominally lives in.
//!
//! For now, the weather. It is real: she and the people in her city are under
//! the same sky, which is the cheapest piece of shared reality there is.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use crate::Prompts;

#[async_trait]
pub trait Senses: Send + Sync + 'static {
    /// Today's weather in a city, or nothing if unknown.
    async fn weather(&self, city: &str, country: &str) -> Option<Forecast>;
}

/// A day's forecast: its WMO weather code, the low and the high in °C, and
/// the chance of rain in percent, if known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Forecast {
    pub code: i64,
    pub low: f64,
    pub high: f64,
    pub rain_chance: Option<i64>,
}

impl Forecast {
    /// The forecast in a few words, the way a glance out of the window
    /// says it, in the words of `words`.
    pub fn words(&self, words: &Prompts) -> String {
        let sky = words.text(match self.code {
            0 => "sky_clear",
            1 | 2 => "sky_partly_cloudy",
            3 => "sky_overcast",
            45 | 48 => "sky_foggy",
            51..=57 => "sky_drizzle",
            61..=67 => "sky_rain",
            71..=77 => "sky_snow",
            80..=82 => "sky_showers",
            85 | 86 => "sky_snow_showers",
            95..=99 => "sky_thunderstorms",
            _ => "sky_unsettled",
        });
        let rain = self
            .rain_chance
            .filter(|p| *p >= 30)
            .map(|p| words.fill("rain_chance", &[("chance", p.to_string())]))
            .unwrap_or_default();
        words.fill(
            "forecast",
            &[
                ("sky", sky.to_owned()),
                ("low", format!("{:.0}", self.low)),
                ("high", format!("{:.0}", self.high)),
                ("rain", rain),
            ],
        )
    }
}

/// No senses at all.
pub struct Numb;

#[async_trait]
impl Senses for Numb {
    async fn weather(&self, _city: &str, _country: &str) -> Option<Forecast> {
        None
    }
}

/// Weather from open-meteo.com. Free, no key.
pub struct OpenMeteo {
    client: reqwest::Client,
    /// City to coordinates, looked up once.
    places: Mutex<HashMap<String, (f64, f64)>>,
}

impl Default for OpenMeteo {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenMeteo {
    pub fn new() -> Self {
        Self {
            client: ghost_net::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            places: Mutex::new(HashMap::new()),
        }
    }

    async fn locate(&self, city: &str, country: &str) -> Option<(f64, f64)> {
        let key = format!("{city},{country}");
        if let Some(found) = self.places.lock().ok()?.get(&key) {
            return Some(*found);
        }
        let mut query = vec![("name", city), ("count", "1"), ("format", "json")];
        if country.len() == 2 {
            query.push(("countryCode", country));
        }
        let v: Value = self
            .client
            .get("https://geocoding-api.open-meteo.com/v1/search")
            .query(&query)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        let hit = v["results"].get(0)?;
        let found = (hit["latitude"].as_f64()?, hit["longitude"].as_f64()?);
        self.places.lock().ok()?.insert(key, found);
        Some(found)
    }
}

#[async_trait]
impl Senses for OpenMeteo {
    async fn weather(&self, city: &str, country: &str) -> Option<Forecast> {
        let (lat, lon) = self.locate(city, country).await?;
        let v: Value = self
            .client
            .get("https://api.open-meteo.com/v1/forecast")
            .query(&[
                ("latitude", lat.to_string()),
                ("longitude", lon.to_string()),
                (
                    "daily",
                    "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max"
                        .to_owned(),
                ),
                ("timezone", "auto".to_owned()),
                ("forecast_days", "1".to_owned()),
            ])
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        let daily = &v["daily"];
        let forecast = Forecast {
            code: daily["weather_code"][0].as_i64()?,
            low: daily["temperature_2m_min"][0].as_f64()?,
            high: daily["temperature_2m_max"][0].as_f64()?,
            rain_chance: daily["precipitation_probability_max"][0].as_i64(),
        };
        debug!("weather in {city}: {forecast:?}");
        Some(forecast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(code: i64, low: f64, high: f64, rain_chance: Option<i64>) -> String {
        Forecast {
            code,
            low,
            high,
            rain_chance,
        }
        .words(&Prompts::shipped())
    }

    #[test]
    fn forecasts_read_like_a_glance_out_the_window() {
        assert_eq!(
            said(61, 9.4, 13.6, Some(80)),
            "rain, 9 to 14°C, 80% chance of rain"
        );
        assert_eq!(said(0, 2.0, 11.0, Some(5)), "clear, 2 to 11°C");
        assert_eq!(said(1234, 0.0, 1.0, None), "unsettled, 0 to 1°C");
    }
}
