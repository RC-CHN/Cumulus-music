use super::PaletteColor;

/// Supplied sampled RGBA only. This cap keeps all histogram populations within
/// the original signed Int16 range and bounds CPU/memory independently of I/O.
pub const MAX_RGBA_PIXELS: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleError {
    IncompletePixel,
    TooManyPixels,
}

impl std::fmt::Display for SampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::IncompletePixel => "sampled RGBA has an incomplete pixel",
            Self::TooManyPixels => "sampled RGBA exceeds the pixel limit",
        })
    }
}

impl std::error::Error for SampleError {}

#[derive(Clone, Copy)]
struct Swatch {
    color: PaletteColor,
    population: u32,
}

fn components(value: u16) -> [u16; 3] {
    [(value >> 10) & 31, (value >> 5) & 31, value & 31]
}

fn color(value: [u16; 3]) -> PaletteColor {
    PaletteColor::rgb(
        (value[0] << 3) as u8,
        (value[1] << 3) as u8,
        (value[2] << 3) as u8,
    )
}

fn allowed(color: PaletteColor) -> bool {
    let [r, g, b] = [color.r, color.g, color.b].map(|v| f64::from(v.unwrap_or_default()) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let light = (max + min) / 2.0;
    let saturation = if max == min {
        0.0
    } else {
        delta / (1.0 - (2.0 * light - 1.0).abs())
    };
    !(light <= 0.1 || (light >= 0.85 && saturation <= 0.1))
}

struct ColorBox {
    lower: usize,
    upper: usize,
    min: [u16; 3],
    max: [u16; 3],
    population: u32,
}

impl ColorBox {
    fn new(lower: usize, upper: usize, colors: &[u16], histogram: &[u32]) -> Self {
        let mut value = Self {
            lower,
            upper,
            min: [u16::MAX; 3],
            max: [0; 3],
            population: 0,
        };
        value.fit(colors, histogram);
        value
    }

    fn fit(&mut self, colors: &[u16], histogram: &[u32]) {
        self.min = [u16::MAX; 3];
        self.max = [0; 3];
        self.population = 0;
        for &entry in &colors[self.lower..=self.upper] {
            self.population += histogram[usize::from(entry)];
            for (i, channel) in components(entry).into_iter().enumerate() {
                self.min[i] = self.min[i].min(channel);
                self.max[i] = self.max[i].max(channel);
            }
        }
    }

    fn volume(&self) -> u32 {
        (0..3)
            .map(|i| u32::from(self.max[i] - self.min[i] + 1))
            .product()
    }

    fn split(&mut self, colors: &mut [u16], histogram: &[u32]) -> Self {
        let lengths = std::array::from_fn::<_, 3, _>(|i| self.max[i] - self.min[i]);
        let axis = if lengths[0] >= lengths[1] && lengths[0] >= lengths[2] {
            0
        } else if lengths[1] >= lengths[0] && lengths[1] >= lengths[2] {
            1
        } else {
            2
        };
        let swap = |value: &mut u16| {
            let mut rgb = components(*value);
            rgb.swap(0, axis);
            *value = (rgb[0] << 10) | (rgb[1] << 5) | rgb[2];
        };
        let slice = &mut colors[self.lower..=self.upper];
        slice.iter_mut().for_each(swap);
        slice.sort_unstable();
        slice.iter_mut().for_each(swap);
        let mut sum = 0;
        let mut split = self.lower;
        for (offset, entry) in slice.iter().enumerate() {
            sum += histogram[usize::from(*entry)];
            if f64::from(sum) >= f64::from(self.population) / 2.0 {
                split = (self.lower + offset).min(self.upper - 1);
                break;
            }
        }
        let other = Self::new(split + 1, self.upper, colors, histogram);
        self.upper = split;
        self.fit(colors, histogram);
        other
    }

    fn average(&self, colors: &[u16], histogram: &[u32]) -> Swatch {
        let mut sums = [0_u32; 3];
        let mut population = 0;
        for &entry in &colors[self.lower..=self.upper] {
            let count = histogram[usize::from(entry)];
            population += count;
            for (i, channel) in components(entry).into_iter().enumerate() {
                sums[i] += count * u32::from(channel);
            }
        }
        Swatch {
            color: color(sums.map(|sum| (f64::from(sum) / f64::from(population)).round() as u16)),
            population,
        }
    }
}

// Exact original min-heap mechanics with the descending-volume comparator.
// std::BinaryHeap tie behavior would change the dominant color for equal boxes.
#[derive(Default)]
struct Heap(Vec<usize>);

impl Heap {
    fn push(&mut self, index: usize, boxes: &[ColorBox]) {
        let mut position = self.0.len();
        self.0.push(index);
        while position > 0 {
            let parent = (position - 1) >> 1;
            if boxes[index].volume() <= boxes[self.0[parent]].volume() {
                break;
            }
            self.0[position] = self.0[parent];
            position = parent;
        }
        self.0[position] = index;
    }

    fn pop(&mut self, boxes: &[ColorBox]) -> Option<usize> {
        let first = *self.0.first()?;
        let last = self.0.pop()?;
        if self.0.is_empty() {
            return Some(first);
        }
        let mut position = 0;
        while position < self.0.len() >> 1 {
            let mut child = 1 + (position << 1);
            let right = child + 1;
            if right < self.0.len() && boxes[self.0[right]].volume() > boxes[self.0[child]].volume()
            {
                child = right;
            }
            if boxes[self.0[child]].volume() <= boxes[last].volume() {
                break;
            }
            self.0[position] = self.0[child];
            position = child;
        }
        self.0[position] = last;
        Some(first)
    }
}

fn quantize(histogram: &[u32], colors: &mut [u16]) -> Vec<Swatch> {
    let mut boxes = vec![ColorBox::new(0, colors.len() - 1, colors, histogram)];
    let mut heap = Heap(vec![0]);
    while heap.0.len() < 16 {
        let Some(index) = heap.pop(&boxes) else {
            break;
        };
        if boxes[index].upper <= boxes[index].lower {
            // Original pops the unsplittable box before breaking.
            break;
        }
        let other = boxes[index].split(colors, histogram);
        let other_index = boxes.len();
        boxes.push(other);
        heap.push(other_index, &boxes);
        heap.push(index, &boxes);
    }
    let mut swatches = Vec::new();
    while let Some(index) = heap.pop(&boxes) {
        let swatch = boxes[index].average(colors, histogram);
        if allowed(swatch.color) {
            swatches.push(swatch);
        }
    }
    if swatches.is_empty() {
        // Both original heaps initially point at the same mutable box. The
        // backup contains its post-split state, not an untouched root clone.
        swatches.push(boxes[0].average(colors, histogram));
    }
    swatches
}

/// M258 dominant swatch on already-sampled RGBA (alpha intentionally ignored).
/// Empty input has no dominant swatch. No target-color scoring is needed by
/// Scene FM: the official consumer asks only for getDominantColor().
pub fn sample_rgba(rgba: &[u8]) -> Result<Option<PaletteColor>, SampleError> {
    if !rgba.len().is_multiple_of(4) {
        return Err(SampleError::IncompletePixel);
    }
    if rgba.len() / 4 > MAX_RGBA_PIXELS {
        return Err(SampleError::TooManyPixels);
    }
    if rgba.is_empty() {
        return Ok(None);
    }
    let mut histogram = vec![0_u32; 32768];
    for pixel in rgba.chunks_exact(4) {
        let index = (usize::from(pixel[0] >> 3) << 10)
            | (usize::from(pixel[1] >> 3) << 5)
            | usize::from(pixel[2] >> 3);
        histogram[index] += 1;
    }
    let mut count = histogram
        .iter()
        .enumerate()
        .filter(|(i, n)| **n > 0 && allowed(color(components(*i as u16))))
        .count();
    if count == 0 {
        count = histogram.iter().filter(|n| **n > 0).count();
    }
    // The original sizes an Int16Array using the allowed count, then writes
    // every positive histogram entry. Out-of-bounds assignments are ignored.
    let mut colors = histogram
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .take(count)
        .map(|(i, _)| i as u16)
        .collect::<Vec<_>>();
    let swatches = if count <= 16 {
        colors
            .iter()
            .map(|value| Swatch {
                color: color(components(*value)),
                population: histogram[usize::from(*value)],
            })
            .collect()
    } else {
        quantize(&histogram, &mut colors)
    };
    let mut dominant = None::<Swatch>;
    for swatch in swatches {
        if dominant.is_none_or(|old| swatch.population > old.population) {
            dominant = Some(swatch);
        }
    }
    Ok(dominant.map(|s| s.color))
}
