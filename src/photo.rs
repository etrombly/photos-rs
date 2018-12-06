use chrono::NaiveDateTime;
use geo::algorithm::haversine_distance::HaversineDistance;
use geo::Point;
use location_history::Location;
use rexiv2::Metadata;
use std::path::PathBuf;

pub struct Photo {
    pub path: PathBuf,
    pub location_name: Option<String>,
    pub location: Option<Point<f32>>,
    pub time: Option<NaiveDateTime>,
}

pub struct TimePhoto<'a>(pub &'a Photo);

impl Photo {
    pub fn new(path: PathBuf) -> Photo {
        let meta = match Metadata::new_from_path(path.clone()) {
            Ok(x) => Some(x),
            Err(_) => None,
        };
        let location = match meta {
            Some(ref x) => Photo::gps_to_point(x.get_gps_info()),
            None => None,
        };
        let time = match meta {
            Some(ref x) => match x.get_tag_string("Exif.Image.DateTime") {
                Ok(y) => match NaiveDateTime::parse_from_str(&y, "%Y:%m:%d %H:%M:%S") {
                    Ok(z) => Some(z),
                    Err(_) => None,
                },
                Err(_) => None,
            },
            None => None,
        };
        Photo {
            path,
            location_name: None,
            location,
            time,
        }
    }

    fn gps_to_point(gps: Option<rexiv2::GpsInfo>) -> Option<Point<f32>> {
        match gps {
            Some(x) => Some(Point::new(x.longitude as f32, x.latitude as f32)),
            None => None,
        }
    }

    pub fn set_location(&mut self, location: Location) {
        self.location = Some(Point::new(location.longitude, location.latitude));
    }
}

impl cogset::Point for Photo {
    fn dist(&self, other: &Photo) -> f64 {
        // returning MAX isn't really correct, but shouldn't throw off the clustering
        match self.location {
            Some(x) => match other.location {
                Some(y) => x.haversine_distance(&y) as f64,
                None => ::std::f64::MAX,
            },
            None => ::std::f64::MAX,
        }
    }
}

impl<'a> cogset::Point for TimePhoto<'a> {
    fn dist(&self, other: &TimePhoto) -> f64 {
        match self.0.time {
            Some(x) => match other.0.time {
                Some(y) => ((x.timestamp() - y.timestamp()) as f64).abs(),
                None => ::std::f64::MAX,
            },
            None => ::std::f64::MAX,
        }
    }
}
