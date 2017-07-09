extern crate chrono;
extern crate rexiv2;
extern crate location_history;

use std::fs::File;
use std::fs::read_dir;
use std::io::prelude::*;
use chrono::NaiveDateTime;
use location_history::Locations;

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
    let photos = read_dir("./photos").unwrap();
    for path in photos {
        let path = path.unwrap().path();
        println!("  Name: {}", path.display());
        let meta = rexiv2::Metadata::new_from_path(path).unwrap();
        println!("  Filetype: {}", meta.get_media_type().unwrap());
        let tag = "Exif.Image.DateTime";
        let gps = meta.get_gps_info();
        if let Ok(time_str) = meta.get_tag_string(tag){
            if let Ok(time) = NaiveDateTime::parse_from_str(&time_str, "%Y:%m:%d %H:%M:%S"){
                println!("  Date: {:?}", time);
                let closest = locations.find_closest(time);
                println!("  closest timestamp: {:?} lat: {} long: {} accuracy: {}", closest.timestamp, closest.latitude, closest.longitude, closest.accuracy);
                println!("  actual location: {:?}", gps);
            }
        }
        println!("");
    }
}