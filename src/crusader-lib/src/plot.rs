use plotters::coord::types::RangedCoordf64;
use plotters::coord::Shift;
use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};
use plotters::style::{register_font, RGBColor};

use std::mem;
use std::path::Path;
use std::time::Duration;

use crate::file_format::{RawLatency, RawPing, RawResult};
use crate::test::{unique, PlotConfig};

const UP_COLOR: RGBColor = RGBColor(37, 83, 169);
const DOWN_COLOR: RGBColor = RGBColor(95, 145, 62);

pub fn register_fonts() {
    register_font("sans-serif", include_bytes!("../Ubuntu-Light.ttf"))
        .map_err(|_| ())
        .unwrap();
}

impl RawResult {
    pub fn to_test_result(&self) -> TestResult {
        let bandwidth_interval = self.config.bandwidth_interval;

        let stream_groups: Vec<_> = self
            .stream_groups
            .iter()
            .map(|group| TestStreamGroup {
                download: group.download,
                both: group.both,
                streams: (0..(group.streams.len()))
                    .map(|i| {
                        let bytes: Vec<_> = (0..=i)
                            .map(|i| to_float(&group.streams[i].to_vec()))
                            .collect();
                        let bytes: Vec<_> = bytes.iter().map(|stream| stream.as_slice()).collect();
                        TestStream {
                            data: sum_bytes(&bytes, bandwidth_interval),
                        }
                    })
                    .collect(),
            })
            .collect();

        let process_bytes = |bytes: Vec<Vec<(u64, u64)>>| -> Vec<(u64, f64)> {
            let bytes: Vec<_> = bytes.iter().map(|stream| to_float(stream)).collect();
            let bytes: Vec<_> = bytes.iter().map(|stream| stream.as_slice()).collect();
            sum_bytes(&bytes, bandwidth_interval)
        };

        let groups: Vec<_> = self
            .stream_groups
            .iter()
            .map(|group| {
                let streams: Vec<_> = group.streams.iter().map(|stream| stream.to_vec()).collect();
                let single = process_bytes(streams);
                (group, single)
            })
            .collect();

        let find = |download, both| {
            groups
                .iter()
                .find(|group| group.0.download == download && group.0.both == both)
                .map(|group| group.1.clone())
        };

        let download_bytes_sum = find(true, false);
        let both_download_bytes_sum = find(true, true);

        let combined_download_bytes: Vec<_> = [
            download_bytes_sum.as_deref(),
            both_download_bytes_sum.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let combined_download_bytes = sum_bytes(&combined_download_bytes, bandwidth_interval);

        let upload_bytes_sum = find(false, false);

        let both_upload_bytes_sum = find(false, true);

        let combined_upload_bytes: Vec<_> = [
            upload_bytes_sum.as_deref(),
            both_upload_bytes_sum.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let combined_upload_bytes = sum_bytes(&combined_upload_bytes, bandwidth_interval);

        let both_bytes = self.both().then(|| {
            sum_bytes(
                &[
                    both_download_bytes_sum.as_deref().unwrap(),
                    both_upload_bytes_sum.as_deref().unwrap(),
                ],
                bandwidth_interval,
            )
        });

        let pings = self.pings.clone();

        TestResult {
            raw_result: self.clone(),
            start: self.start,
            duration: self.duration,
            pings,
            both_bytes,
            both_download_bytes: both_download_bytes_sum,
            both_upload_bytes: both_upload_bytes_sum,
            download_bytes: download_bytes_sum,
            upload_bytes: upload_bytes_sum,
            combined_download_bytes,
            combined_upload_bytes,
            stream_groups,
        }
    }
}

pub struct TestStream {
    pub data: Vec<(u64, f64)>,
}

pub struct TestStreamGroup {
    pub download: bool,
    pub both: bool,
    pub streams: Vec<TestStream>,
}

pub struct TestResult {
    pub raw_result: RawResult,
    pub start: Duration,
    pub duration: Duration,
    pub download_bytes: Option<Vec<(u64, f64)>>,
    pub upload_bytes: Option<Vec<(u64, f64)>>,
    pub combined_download_bytes: Vec<(u64, f64)>,
    pub combined_upload_bytes: Vec<(u64, f64)>,
    pub both_download_bytes: Option<Vec<(u64, f64)>>,
    pub both_upload_bytes: Option<Vec<(u64, f64)>>,
    pub both_bytes: Option<Vec<(u64, f64)>>,
    pub pings: Vec<RawPing>,
    pub stream_groups: Vec<TestStreamGroup>,
}

pub fn save_graph(config: &PlotConfig, result: &TestResult, name: &str) -> String {
    let file = unique(name, "png");
    save_graph_to_path(file.as_ref(), config, result);
    file
}

pub fn save_graph_to_path(path: &Path, config: &PlotConfig, result: &TestResult) {
    let mut bandwidth = Vec::new();

    result.both_bytes.as_ref().map(|both_bytes| {
        bandwidth.push((
            "Both",
            RGBColor(149, 96, 153),
            to_rates(both_bytes),
            vec![both_bytes.as_slice()],
        ));
    });

    if result.upload_bytes.is_some() || result.both_upload_bytes.is_some() {
        bandwidth.push((
            "Upload",
            UP_COLOR,
            to_rates(&result.combined_upload_bytes),
            [
                result.upload_bytes.as_deref(),
                result.both_upload_bytes.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        ));
    }

    if result.download_bytes.is_some() || result.both_download_bytes.is_some() {
        bandwidth.push((
            "Download",
            DOWN_COLOR,
            to_rates(&result.combined_download_bytes),
            [
                result.download_bytes.as_deref(),
                result.both_download_bytes.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        ));
    }

    graph(
        path,
        config,
        result,
        &result.pings,
        &bandwidth,
        result.start.as_secs_f64(),
        result.duration.as_secs_f64(),
    );
}

pub fn float_max(iter: impl Iterator<Item = f64>) -> f64 {
    let mut max = iter.fold(f64::NAN, f64::max);

    if max.is_nan() {
        max = 100.0;
    }

    max
}

fn to_float(stream: &[(u64, u64)]) -> Vec<(u64, f64)> {
    stream.iter().map(|(t, v)| (*t, *v as f64)).collect()
}

pub fn to_rates(stream: &[(u64, f64)]) -> Vec<(u64, f64)> {
    let mut result: Vec<(u64, f64)> = (0..stream.len())
        .map(|i| {
            let rate = if i > 0 {
                let bytes = stream[i].1 - stream[i - 1].1;
                let duration = Duration::from_micros(stream[i].0 - stream[i - 1].0);
                let mbits = (bytes as f64 * 8.0) / (1000.0 * 1000.0);
                mbits / duration.as_secs_f64()
            } else {
                0.0
            };
            (stream[i].0, rate)
        })
        .collect();

    // Insert dummy zero points for nicer graphs
    if !result.is_empty() {
        result.first().unwrap().0.checked_sub(1).map(|first| {
            result.insert(0, (first, 0.0));
        });
        result.push((result.last().unwrap().0 + 1, 0.0));
    }

    result
}

fn sum_bytes(input: &[&[(u64, f64)]], interval: Duration) -> Vec<(u64, f64)> {
    let interval = interval.as_micros() as u64;

    let bandwidth: Vec<_> = input
        .iter()
        .map(|stream| interpolate(stream, interval))
        .collect();

    let min = bandwidth
        .iter()
        .map(|stream| stream.first().map(|e| e.0).unwrap_or(0))
        .min()
        .unwrap_or(0);

    let max = bandwidth
        .iter()
        .map(|stream| stream.last().map(|e| e.0).unwrap_or(0))
        .max()
        .unwrap_or(0);

    let mut data = Vec::new();

    for point in (min..=max).step_by(interval as usize) {
        let value = bandwidth
            .iter()
            .map(
                |stream| match stream.binary_search_by_key(&point, |e| e.0) {
                    Ok(i) => stream[i].1,
                    Err(0) => 0.0,
                    Err(i) if i == stream.len() => stream.last().unwrap().1,
                    _ => panic!("unexpected index"),
                },
            )
            .sum();
        data.push((point, value));
    }

    data
}

fn interpolate(input: &[(u64, f64)], interval: u64) -> Vec<(u64, f64)> {
    if input.is_empty() {
        return Vec::new();
    }

    let min = input.first().unwrap().0 / interval * interval;
    let max = (input.last().unwrap().0 + interval - 1) / interval * interval;

    let mut data = Vec::new();

    for point in (min..=max).step_by(interval as usize) {
        let i = input.partition_point(|e| e.0 < point);
        let value = if i == input.len() {
            input.last().unwrap().1
        } else if input[i].0 == point || i == 0 {
            input[i].1
        } else {
            let len = input[i].0 - input[i - 1].0;
            if len == 0 {
                input[i].1
            } else {
                let ratio = (point - input[i - 1].0) as f64 / len as f64;
                let delta = input[i].1 - input[i - 1].1;
                input[i - 1].1 + delta * ratio
            }
        };
        data.push((point, value));
    }

    data
}

fn new_chart<'a, 'b, 'c>(
    duration: f64,
    padding_bottom: Option<i32>,
    max: f64,
    label: &'b str,
    x_label: Option<&'b str>,
    area: &'a DrawingArea<BitMapBackend<'c>, Shift>,
) -> ChartContext<'a, BitMapBackend<'c>, Cartesian2d<RangedCoordf64, RangedCoordf64>> {
    let font = (FontFamily::SansSerif, 16);

    let mut chart = ChartBuilder::on(area)
        .margin(6)
        .set_label_area_size(LabelAreaPosition::Left, 100)
        .set_label_area_size(LabelAreaPosition::Right, 100)
        .set_label_area_size(LabelAreaPosition::Bottom, padding_bottom.unwrap_or(20))
        .build_cartesian_2d(0.0..duration, 0.0..max)
        .unwrap();

    chart
        .plotting_area()
        .fill(&RGBColor(248, 248, 248))
        .unwrap();

    let mut mesh = chart.configure_mesh();

    mesh.disable_x_mesh().disable_y_mesh();

    if x_label.is_none() {
        mesh.x_labels(20).y_labels(10);
    } else {
        mesh.x_labels(0).y_labels(0);
    }

    mesh.x_label_style(font).y_label_style(font).y_desc(label);

    if let Some(label) = x_label {
        mesh.x_desc(label);
    }

    mesh.draw().unwrap();

    chart
}

fn legends<'a, 'b: 'a>(
    chart: &mut ChartContext<'a, BitMapBackend<'b>, Cartesian2d<RangedCoordf64, RangedCoordf64>>,
) {
    let font = (FontFamily::SansSerif, 16);

    chart
        .configure_series_labels()
        .background_style(&WHITE.mix(0.8))
        .label_font(font)
        .border_style(&BLACK)
        .draw()
        .unwrap();
}

fn latency(
    result: &TestResult,
    pings: &[RawPing],
    start: f64,
    duration: f64,
    area: &DrawingArea<BitMapBackend, Shift>,
    packet_loss_area: &DrawingArea<BitMapBackend, Shift>,
) {
    let max_latency = pings
        .iter()
        .filter_map(|d| d.latency)
        .filter_map(|latency| latency.total)
        .max()
        .unwrap_or(Duration::from_millis(100))
        .as_secs_f64() as f64
        * 1000.0;

    let max_latency = max_latency * 1.05;

    // Latency

    let mut chart = new_chart(duration, None, max_latency, "Latency (ms)", None, area);

    let mut draw_latency =
        |color: RGBColor, name: &str, get_latency: fn(&RawLatency) -> Option<Duration>| {
            let mut data = Vec::new();

            let flush = |data: &mut Vec<_>| {
                let data = mem::take(data);

                if data.len() == 1 {
                    chart
                        .plotting_area()
                        .draw(&Circle::new(data[0], 1, color.filled()))
                        .unwrap();
                } else {
                    chart
                        .plotting_area()
                        .draw(&PathElement::new(data, color))
                        .unwrap();
                }
            };

            for ping in pings {
                match &ping.latency {
                    Some(latency) => match get_latency(latency) {
                        Some(latency) => {
                            let x = ping.sent.as_secs_f64() - start;
                            let y = latency.as_secs_f64() * 1000.0;

                            data.push((x, y));
                        }
                        None => {
                            flush(&mut data);
                        }
                    },
                    None => {
                        flush(&mut data);
                    }
                }
            }

            flush(&mut data);

            chart
                .draw_series(LineSeries::new(std::iter::empty(), color))
                .unwrap()
                .label(name)
                .legend(move |(x, y)| {
                    Rectangle::new([(x, y - 5), (x + 18, y + 3)], color.filled())
                });
        };

    draw_latency(UP_COLOR, "Up", |latency| Some(latency.up));

    draw_latency(DOWN_COLOR, "Down", |latency| latency.down());

    draw_latency(RGBColor(50, 50, 50), "Total", |latency| latency.total);

    legends(&mut chart);

    // Packet loss

    let chart = new_chart(
        duration,
        Some(30),
        1.0,
        "Packet loss",
        Some("Elapsed time (seconds)"),
        packet_loss_area,
    );

    for ping in pings {
        let x = ping.sent.as_secs_f64() - start;
        if ping.latency.and_then(|latency| latency.total).is_none() {
            let bold_size = 0.1111;
            let (color, s, e, bold) = if result.raw_result.version >= 2 {
                if ping.latency.is_none() {
                    (UP_COLOR, 0.0, 0.5, Some(0.0 + bold_size))
                } else {
                    (DOWN_COLOR, 1.0, 0.5, Some(1.0 - bold_size))
                }
            } else {
                (RGBColor(193, 85, 85), 0.0, 1.0, None)
            };
            chart
                .plotting_area()
                .draw(&PathElement::new(vec![(x, s), (x, e)], color))
                .unwrap();
            bold.map(|bold| {
                chart
                    .plotting_area()
                    .draw(&PathElement::new(
                        vec![(x, s), (x, bold)],
                        color.stroke_width(2),
                    ))
                    .unwrap();
            });
        }
    }

    chart
        .plotting_area()
        .draw(&PathElement::new(vec![(0.0, 1.0), (duration, 1.0)], BLACK))
        .unwrap();
}

fn plot_split_bandwidth(
    download: bool,
    result: &TestResult,
    start: f64,
    duration: f64,
    area: &DrawingArea<BitMapBackend, Shift>,
) {
    let groups: Vec<_> = result
        .stream_groups
        .iter()
        .filter(|group| group.download == download)
        .map(|group| TestStreamGroup {
            download,
            both: group.both,
            streams: group
                .streams
                .iter()
                .map(|stream| TestStream {
                    data: to_rates(&stream.data),
                })
                .collect(),
        })
        .collect();

    let max_bandwidth = float_max(
        groups
            .iter()
            .flat_map(|group| group.streams.last().unwrap().data.iter())
            .map(|e| e.1),
    );

    let max_bandwidth = max_bandwidth * 1.05;

    let mut chart = new_chart(
        duration,
        None,
        max_bandwidth,
        if download {
            "Download (Mbps)"
        } else {
            "Upload (Mbps)"
        },
        None,
        area,
    );

    for group in groups {
        for i in 0..(group.streams.len()) {
            let main = i == group.streams.len() - 1;
            let color = if download {
                if main {
                    DOWN_COLOR
                } else {
                    if i & 1 == 0 {
                        RGBColor(188, 203, 177)
                    } else {
                        RGBColor(215, 223, 208)
                    }
                }
            } else {
                if main {
                    UP_COLOR
                } else {
                    if i & 1 == 0 {
                        RGBColor(159, 172, 202)
                    } else {
                        RGBColor(211, 217, 231)
                    }
                }
            };
            chart
                .draw_series(LineSeries::new(
                    group.streams[i].data.iter().map(|(time, rate)| {
                        (Duration::from_micros(*time).as_secs_f64() - start, *rate)
                    }),
                    color,
                ))
                .unwrap();
        }
    }
}

fn plot_bandwidth(
    bandwidth: &[(&str, RGBColor, Vec<(u64, f64)>, Vec<&[(u64, f64)]>)],
    start: f64,
    duration: f64,
    area: &DrawingArea<BitMapBackend, Shift>,
) {
    let max_bandwidth = float_max(bandwidth.iter().flat_map(|list| list.2.iter()).map(|e| e.1));

    let max_bandwidth = max_bandwidth * 1.05;

    let mut chart = new_chart(
        duration,
        None,
        max_bandwidth,
        "Bandwidth (Mbps)",
        None,
        area,
    );

    for (name, color, rates, _) in bandwidth {
        chart
            .draw_series(LineSeries::new(
                rates.iter().map(|(time, rate)| {
                    (Duration::from_micros(*time).as_secs_f64() - start, *rate)
                }),
                color,
            ))
            .unwrap()
            .label(*name)
            .legend(move |(x, y)| Rectangle::new([(x, y - 5), (x + 18, y + 3)], color.filled()));
    }

    legends(&mut chart);
}

pub(crate) fn bytes_transferred(
    bandwidth: &[(&str, RGBColor, Vec<(u64, f64)>, Vec<&[(u64, f64)]>)],
    start: f64,
    duration: f64,
    area: &DrawingArea<BitMapBackend, Shift>,
) {
    let max_bytes = float_max(
        bandwidth
            .iter()
            .flat_map(|list| list.3.iter())
            .flat_map(|list| list.iter())
            .map(|e| e.1),
    );

    let max_bytes = max_bytes / (1024.0 * 1024.0 * 1024.0);

    let max_bytes = max_bytes * 1.05;

    let mut chart = new_chart(
        duration,
        Some(50),
        max_bytes,
        "Data transferred (GiB)",
        None,
        area,
    );

    for (name, color, _, bytes) in bandwidth {
        for (i, bytes) in bytes.iter().enumerate() {
            let series = chart
                .draw_series(LineSeries::new(
                    bytes.iter().map(|(time, bytes)| {
                        (
                            Duration::from_micros(*time).as_secs_f64() - start,
                            *bytes / (1024.0 * 1024.0 * 1024.0),
                        )
                    }),
                    &color,
                ))
                .unwrap();

            if i == 0 {
                series.label(*name).legend(move |(x, y)| {
                    Rectangle::new([(x, y - 5), (x + 18, y + 3)], color.filled())
                });
            }
        }
    }

    legends(&mut chart);
}

pub(crate) fn graph(
    path: &Path,
    config: &PlotConfig,
    result: &TestResult,
    pings: &[RawPing],
    bandwidth: &[(&str, RGBColor, Vec<(u64, f64)>, Vec<&[(u64, f64)]>)],
    start: f64,
    duration: f64,
) {
    let width = config.width.unwrap_or(1280) as u32;

    let root =
        BitMapBackend::new(path, (width, config.height.unwrap_or(720) as u32)).into_drawing_area();

    root.fill(&WHITE).unwrap();

    let style: TextStyle = (FontFamily::SansSerif, 26).into();

    let small_style: TextStyle = (FontFamily::SansSerif, 14).into();

    let lines = 2;

    let text_height = (root.estimate_text_size("Wg", &small_style).unwrap().1 as i32 + 5) * lines;

    let center = text_height / 2 + 10;

    root.draw_text(
        "Latency under load",
        &style.pos(Pos::new(HPos::Center, VPos::Center)),
        (width as i32 / 2, center),
    )
    .unwrap();

    if result.raw_result.version >= 1 {
        let top_margin = 10;
        root.draw_text(
            &format!(
                "Connections: {} over IPv{}",
                result.raw_result.streams(),
                if result.raw_result.ipv6 { 6 } else { 4 },
            ),
            &small_style.pos(Pos::new(HPos::Left, VPos::Top)),
            (100, top_margin + text_height / lines),
        )
        .unwrap();

        root.draw_text(
            &format!(
                "Stagger: {} s",
                result.raw_result.config.stagger.as_secs_f64(),
            ),
            &small_style.pos(Pos::new(HPos::Left, VPos::Top)),
            (100 + 180, top_margin + text_height / lines),
        )
        .unwrap();

        root.draw_text(
            &format!(
                "Load duration: {:.2} s",
                result.raw_result.config.load_duration.as_secs_f64(),
            ),
            &small_style.pos(Pos::new(HPos::Left, VPos::Top)),
            (100, top_margin),
        )
        .unwrap();

        root.draw_text(
            &format!(
                "Server latency: {:.2} ms",
                result.raw_result.server_latency.as_secs_f64() * 1000.0,
            ),
            &small_style.pos(Pos::new(HPos::Left, VPos::Top)),
            (100 + 180, top_margin),
        )
        .unwrap();

        root.draw_text(
            &result.raw_result.generated_by,
            &small_style.pos(Pos::new(HPos::Right, VPos::Center)),
            (width as i32 - 100, center),
        )
        .unwrap();
    }

    let root = root.split_vertically(text_height + 10).1;

    let (root, loss) = root.split_vertically(root.relative_to_height(1.0) - 70.0);

    let mut charts = 1;

    if result.raw_result.streams() > 0 {
        if config.split_bandwidth {
            if result.raw_result.download() || result.raw_result.both() {
                charts += 1
            }
            if result.raw_result.upload() || result.raw_result.both() {
                charts += 1
            }
        } else {
            charts += 1
        }
        if config.transferred {
            charts += 1
        }
    }

    let areas = root.split_evenly((charts, 1));

    // Scale to fit the legend
    let duration = duration * 1.08;

    let mut chart_index = 0;

    if result.raw_result.streams() > 0 {
        if config.split_bandwidth {
            if result.raw_result.download() || result.raw_result.both() {
                plot_split_bandwidth(true, result, start, duration, &areas[chart_index]);
                chart_index += 1;
            }
            if result.raw_result.upload() || result.raw_result.both() {
                plot_split_bandwidth(false, result, start, duration, &areas[chart_index]);
                chart_index += 1;
            }
        } else {
            plot_bandwidth(bandwidth, start, duration, &areas[chart_index]);
            chart_index += 1;
        }
    }

    latency(result, pings, start, duration, &areas[chart_index], &loss);
    chart_index += 1;

    if result.raw_result.streams() > 0 && config.transferred {
        bytes_transferred(bandwidth, start, duration, &areas[chart_index]);
        #[allow(unused_assignments)]
        {
            chart_index += 1;
        }
    }

    root.present().expect("Unable to write plot to file");
}
