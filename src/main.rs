extern crate chrono;
extern crate rexiv2;
extern crate location_history;
extern crate geo;
extern crate cogset;
extern crate walkdir;

use rexiv2::Metadata;
use std::fs::File;
use std::io::prelude::*;
use std::path::PathBuf;
use chrono::NaiveDateTime;
use location_history::Locations;
use geo::Point;
use geo::algorithm::haversine_distance::HaversineDistance;
use cogset::{Dbscan, BruteScan};
use walkdir::WalkDir;

#[derive(Debug)]
struct Photo {
    path: PathBuf,
    meta: Option<Metadata>,
    loc: Option<Point<f64>>,
    time: Option<NaiveDateTime>,
}

struct TimePhoto<'a>(&'a Photo);

impl Photo {
    pub fn new(path: PathBuf) -> Photo {
        let meta = match Metadata::new_from_path(path.clone()) {
            Ok(x) => Some(x),
            Err(_) => None,
        };
        let loc = match &meta {
            &Some(ref x) => gps_to_point(x.get_gps_info()),
            &None => None,
        };
        let time = match &meta {
            &Some(ref x) => {
                match x.get_tag_string("Exif.Image.DateTime") {
                    Ok(y) => {
                        match NaiveDateTime::parse_from_str(&y, "%Y:%m:%d %H:%M:%S") {
                            Ok(z) => Some(z),
                            Err(_) => None,
                        }
                    }
                    Err(_) => None,
                }
            }   
            &None => None,
        };
        Photo {
            path,
            meta,
            loc,
            time,
        }
    }
}

impl cogset::Point for Photo {
    fn dist(&self, other: &Photo) -> f64 {
        // returning MAX isn't really correct, but shouldn't throw off the clustering
        match self.loc {
            Some(x) => {
                match other.loc {
                    Some(y) => x.haversine_distance(&y),
                    None => std::f64::MAX,
                }
            }
            None => std::f64::MAX,
        }
    }
}

impl<'a> cogset::Point for TimePhoto<'a> {
    fn dist(&self, other: &TimePhoto) -> f64 {
        match self.0.time {
            Some(x) => {
                match other.0.time {
                    Some(y) => ((x.timestamp() - y.timestamp()) as f64).abs(),
                    None => std::f64::MAX,
                }
            }
            None => std::f64::MAX,
        }
    }
}

fn gps_to_point(gps: Option<rexiv2::GpsInfo>) -> Option<Point<f64>> {
    match gps {
        Some(x) => Some(Point::new(x.latitude, x.longitude)),
        None => None,
    }
}

fn main() {
    let mut contents = String::new();
    File::open("LocationHistory.json")
        .unwrap()
        .read_to_string(&mut contents)
        .unwrap();
    let locations: Locations = Locations::new(&contents);
    println!("Loaded  {} timestamps", locations.locations.len());
    println!(
        "  from {} to {}",
        locations.locations[locations.locations.len() - 1]
            .timestamp
            .format("%Y-%m-%d %H:%M:%S"),
        locations.locations[0].timestamp.format("%Y-%m-%d %H:%M:%S")
    );
    println!(
        "  {} seconds average between timestamps\n",
        locations.average_time()
    );

    println!("Scanning photos");
    let photos = read_directory("/home/eric/pictures/");
    println!("Found {} photos", photos.len());

    for photo in &photos {
        println!("  Name: {}", photo.path.display());
        if let Some(time) = photo.time {
            println!("  Date: {:?}", time);
            if let Some(closest) = locations.find_closest(time) {
                println!(
                    "  closest timestamp: {:?} lat: {} long: {} accuracy: {}",
                    closest.timestamp,
                    closest.latitude,
                    closest.longitude,
                    closest.accuracy
                );
                if let Some(x) = photo.loc {
                    println!(
                        "  distance error meters: {:.2}",
                        x.haversine_distance(&Point::new(
                            closest.latitude as f64,
                            closest.longitude as f64,
                        ))
                    );
                }
            }
            if let Some(x) = photo.loc {
                println!("  actual location: {:?}", x);
            }
        }
        println!("");
    }

    let scanner = BruteScan::new(&photos);
    let mut dbscan = Dbscan::new(scanner, 1000.0, 3);
    let clusters = dbscan.by_ref().collect::<Vec<_>>();
    for cluster in clusters {
        println!("Cluster located near {:?}", photos[cluster[0]].loc);
        for photo in cluster {
            print!("{:?} ", photos[photo].path);
        }
        println!("\n");
    }

    let timephotos = photos.iter().map(|x| TimePhoto(x)).collect::<Vec<_>>();
    let timescanner = BruteScan::new(&timephotos);
    let mut timedbscan = Dbscan::new(timescanner, 600.0, 10);
    let timeclusters = timedbscan.by_ref().collect::<Vec<_>>();
    for cluster in timeclusters {
        println!("Cluster located at {:?}", timephotos[cluster[0]].0.time);
        for photo in cluster {
            print!("{:?} ", timephotos[photo].0.path);
        }
        println!("\n");
    }
}

fn read_directory(dir: &str) -> Vec<Photo> {
    let files = WalkDir::new(dir).into_iter().filter_map(|e| e.ok());
    let files = files.filter(|x| Metadata::new_from_path(x.path()).is_ok());
    files.map(|x| Photo::new(x.path().to_path_buf())).collect()
}
