//! Calculs sur les images, sans caméra ni modèle : décodage des sorties de YuNet,
//! alignement du visage pour SFace, comparaison de deux empreintes. Tout est testé.

/// Image en niveaux de gris, une valeur par pixel, ligne après ligne.
#[derive(Debug, Clone, PartialEq)]
pub struct Gray {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Gray {
    /// Luminosité moyenne (0 à 255) : la lampe infrarouge met une seconde à s'allumer, les
    /// premières images sont presque noires.
    pub fn brightness(&self) -> f32 {
        if self.pixels.is_empty() {
            return 0.0;
        }
        let total: u64 = self.pixels.iter().map(|&value| u64::from(value)).sum();
        total as f32 / self.pixels.len() as f32
    }

    /// Valeur à (x, y) interpolée, 0 hors de l'image.
    fn sample(&self, x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |x: f32, y: f32| -> f32 {
            if x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32 {
                return 0.0;
            }
            f32::from(self.pixels[y as usize * self.width + x as usize])
        };
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
        let bottom = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// Visage trouvé : cadre (x, y, largeur, hauteur), confiance, et cinq points (œil droit,
/// œil gauche, nez, coin droit puis gauche de la bouche).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub frame: [f32; 4],
    pub score: f32,
    pub landmarks: [[f32; 2]; 5],
}

/// Pas des trois grilles de YuNet.
const STRIDES: [usize; 3] = [8, 16, 32];

/// Meilleur visage des sorties de YuNet pour une entrée carrée de `size` pixels. Les
/// sorties sont, dans l'ordre : `cls`, `obj`, `bbox`, `kps` pour chaque pas (8, 16, 32).
pub fn best_face(outputs: &[&[f32]], size: usize) -> Option<Face> {
    if outputs.len() != 12 {
        return None;
    }
    let mut best: Option<Face> = None;
    for (level, stride) in STRIDES.into_iter().enumerate() {
        let (cls, obj, bbox, kps) = (
            outputs[level],
            outputs[3 + level],
            outputs[6 + level],
            outputs[9 + level],
        );
        let columns = size / stride;
        for index in 0..cls.len() {
            let score = (cls[index].clamp(0.0, 1.0) * obj[index].clamp(0.0, 1.0)).sqrt();
            if best.is_some_and(|face| face.score >= score) {
                continue;
            }
            let (row, column) = ((index / columns) as f32, (index % columns) as f32);
            let s = stride as f32;
            let b = &bbox[index * 4..index * 4 + 4];
            let (cx, cy) = ((column + b[0]) * s, (row + b[1]) * s);
            let (w, h) = (b[2].exp() * s, b[3].exp() * s);
            let k = &kps[index * 10..index * 10 + 10];
            let mut landmarks = [[0.0; 2]; 5];
            for (point, landmark) in landmarks.iter_mut().enumerate() {
                *landmark = [(k[point * 2] + column) * s, (k[point * 2 + 1] + row) * s];
            }
            best = Some(Face {
                frame: [cx - w / 2.0, cy - h / 2.0, w, h],
                score,
                landmarks,
            });
        }
    }
    best
}

/// Points de référence d'un visage aligné de 112 × 112 pour SFace (ceux d'OpenCV).
const TEMPLATE: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

/// Côté du visage aligné.
pub const ALIGNED: usize = 112;

/// Similitude (rotation, échelle, translation) qui envoie `from` sur `to` au mieux, au
/// sens des moindres carrés : `[a, b, tx, ty]` pour x' = a·x − b·y + tx, y' = b·x + a·y + ty.
fn similarity(from: &[[f32; 2]; 5], to: &[[f32; 2]; 5]) -> [f32; 4] {
    let n = from.len() as f32;
    let mean = |points: &[[f32; 2]; 5]| {
        let (sx, sy) = points
            .iter()
            .fold((0.0, 0.0), |(sx, sy), p| (sx + p[0], sy + p[1]));
        [sx / n, sy / n]
    };
    let (mf, mt) = (mean(from), mean(to));
    let (mut dot, mut cross, mut norm) = (0.0, 0.0, 0.0);
    for (f, t) in from.iter().zip(to) {
        let (fx, fy) = (f[0] - mf[0], f[1] - mf[1]);
        let (tx, ty) = (t[0] - mt[0], t[1] - mt[1]);
        dot += fx * tx + fy * ty;
        cross += fx * ty - fy * tx;
        norm += fx * fx + fy * fy;
    }
    if norm == 0.0 {
        return [1.0, 0.0, 0.0, 0.0];
    }
    let (a, b) = (dot / norm, cross / norm);
    [
        a,
        b,
        mt[0] - (a * mf[0] - b * mf[1]),
        mt[1] - (b * mf[0] + a * mf[1]),
    ]
}

/// Visage de `image` redressé et recadré en 112 × 112, d'après ses cinq points.
pub fn align(image: &Gray, face: &Face) -> Gray {
    // Transformation inverse : pour chaque pixel du visage aligné, sa place dans l'image.
    let [a, b, tx, ty] = similarity(&TEMPLATE, &face.landmarks);
    let mut pixels = Vec::with_capacity(ALIGNED * ALIGNED);
    for y in 0..ALIGNED {
        for x in 0..ALIGNED {
            let (x, y) = (x as f32, y as f32);
            let sx = a * x - b * y + tx;
            let sy = b * x + a * y + ty;
            pixels.push(image.sample(sx, sy).round().clamp(0.0, 255.0) as u8);
        }
    }
    Gray {
        width: ALIGNED,
        height: ALIGNED,
        pixels,
    }
}

/// Empreinte ramenée à une longueur de 1.
pub fn normalized(mut vector: Vec<f32>) -> Vec<f32> {
    let length = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length > 0.0 {
        for value in &mut vector {
            *value /= length;
        }
    }
    vector
}

/// Ressemblance de deux empreintes normalisées (cosinus, de −1 à 1).
pub fn similarity_of(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_le_meilleur_visage() {
        // Grille de pas 32 sur une entrée de 64 : 2 × 2 cases ; la case (1, 0) est la bonne.
        let size = 64;
        let (n8, n16, n32) = (64, 16, 4);
        let zeros = |n: usize| vec![0.0; n];
        let mut cls32 = zeros(n32);
        let mut obj32 = zeros(n32);
        cls32[1] = 0.81;
        obj32[1] = 1.0;
        let mut bbox32 = zeros(n32 * 4);
        bbox32[4..8].copy_from_slice(&[0.5, 0.5, 0.0, 0.0]);
        let kps32 = zeros(n32 * 10);
        let (cls8, obj8, bbox8, kps8) = (zeros(n8), zeros(n8), zeros(n8 * 4), zeros(n8 * 10));
        let (cls16, obj16, bbox16, kps16) = (zeros(n16), zeros(n16), zeros(n16 * 4), zeros(n16 * 10));
        let outputs: Vec<&[f32]> = vec![
            &cls8, &cls16, &cls32, &obj8, &obj16, &obj32, &bbox8, &bbox16, &bbox32, &kps8, &kps16, &kps32,
        ];
        let face = best_face(&outputs, size).expect("trouvé");
        assert!((face.score - 0.9).abs() < 1e-4);
        // Centre (1 + 0,5) × 32 = 48 en x, 0,5 × 32 = 16 en y ; côté e⁰ × 32 = 32.
        assert_eq!(face.frame, [32.0, 0.0, 32.0, 32.0]);
        assert_eq!(face.landmarks[0], [32.0, 0.0]);
    }

    #[test]
    fn l_alignement_retrouve_les_points_de_reference() {
        // Visage deux fois plus grand et décalé : la similitude doit l'inverser exactement.
        let face: [[f32; 2]; 5] = TEMPLATE.map(|[x, y]| [x * 2.0 + 10.0, y * 2.0 + 20.0]);
        let [a, b, tx, ty] = similarity(&TEMPLATE, &face);
        assert!((a - 2.0).abs() < 1e-4 && b.abs() < 1e-4);
        assert!((tx - 10.0).abs() < 1e-3 && (ty - 20.0).abs() < 1e-3);
    }

    #[test]
    fn deux_empreintes_identiques_se_ressemblent_tout_a_fait() {
        let a = normalized(vec![3.0, 4.0]);
        assert!((similarity_of(&a, &a) - 1.0).abs() < 1e-6);
        let b = normalized(vec![-4.0, 3.0]);
        assert!(similarity_of(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn luminosite_moyenne() {
        let image = Gray {
            width: 2,
            height: 1,
            pixels: vec![0, 100],
        };
        assert_eq!(image.brightness(), 50.0);
    }
}
