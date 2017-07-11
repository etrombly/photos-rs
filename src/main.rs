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
    loc: Option<Point<f64>>,
    meta: Option<Metadata>,
}

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
        Photo{path, loc, meta}
    }
}

impl cogset::Point for Photo {
    fn dist(&self, other: &Photo) -> f64 {
        // returning MAX isn't really correct, but shouldn't throw off the clustering
        match self.loc{
            Some(x) => {
                match other.loc {
                    Some(y) => x.haversine_distance(&y),
                    None => std::f64::MAX,
                }
            },
            None => std::f64::MAX,
        }
    }
}

fn gps_to_point(gps: Option<rexiv2::GpsInfo>) -> Option<Point<f64>> {
    match gps {
        Some(x) => {
            Some(Point::new(x.latitude, x.longitude))
        },
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
    println!("  from {} to {}",
             locations.locations[locations.locations.len() - 1]
                 .timestamp
                 .format("%Y-%m-%d %H:%M:%S"),
             locations.locations[0]
                 .timestamp
                 .format("%Y-%m-%d %H:%M:%S"));
    println!("  {} seconds average between timestamps\n",
             locations.average_time());
    
    println!("Scanning photos");
    let photos = read_directory("photos");

    for photo in &photos {
        println!("  Name: {}", photo.path.display());
        if let Some(ref meta) = photo.meta {
            println!("  Filetype: {}", meta.get_media_type().unwrap());
            if let Ok(time_str) = meta.get_tag_string("Exif.Image.DateTime"){
                if let Ok(time) = NaiveDateTime::parse_from_str(&time_str, "%Y:%m:%d %H:%M:%S"){
                    println!("  Date: {:?}", time);
                    if let Some(closest) = locations.find_closest(time){
                        println!("  closest timestamp: {:?} lat: {} long: {} accuracy: {}", closest.timestamp, closest.latitude, closest.longitude, closest.accuracy);
                        match photo.loc {
                            Some(x) => println!("  distance error meters: {:.2}", 
                                                x.haversine_distance(&Point::new(closest.latitude as f64, closest.longitude as f64))),
                            _ => {},
                        }
                    }
                    match photo.loc {
                        Some(x) => println!("  actual location: {:?}", x),
                        _ => {},
                    }
                }
            }
            println!("");
        }
    }
    let scanner = BruteScan::new(&photos);
    let mut dbscan = Dbscan::new(scanner, 500.0, 3);
    let clusters = dbscan.by_ref().collect::<Vec<_>>();
    for cluster in clusters {
        for photo in cluster {
            print!("{:?} ", photos[photo].path);
        }
        println!("\n");
    }
}

fn read_directory(dir: &str) -> Vec<Photo> {
    let files = WalkDir::new(dir).into_iter().filter_map(|e| e.ok());
    let files = files.filter(|x| Metadata::new_from_path(x.path())
                                .is_ok());
    files.map(|x| Photo::new(x.path().to_path_buf())).collect()
}