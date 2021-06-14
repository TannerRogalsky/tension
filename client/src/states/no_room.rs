use super::StateContext;
use solstice_2d::Draw;

#[derive(Debug, Default)]
pub struct NoRoom {
    elapsed: std::time::Duration,
}

impl NoRoom {
    pub fn update(&mut self, dt: std::time::Duration) {
        self.elapsed += dt;
    }

    pub fn render(&self, mut ctx: StateContext) {
        let width = ctx.g.gfx().viewport().width() as f32;
        let height = ctx.g.gfx().viewport().height() as f32;

        ctx.g.clear([1., 0., 0., 1.]);

        let count = 10;
        let geometry = solstice_2d::Circle {
            x: 0.0,
            y: 0.0,
            radius: width * 0.05,
            segments: 30,
        };
        for i in 0..count {
            let r = i as f32 / count as f32;
            let phi = r * std::f32::consts::TAU + self.elapsed.as_secs_f32();
            let (x, y) = phi.sin_cos();

            ctx.g.draw(solstice_2d::Circle {
                x: x * width * 0.2 + width / 2.,
                y: y * height * 0.2 + height / 2.,
                ..geometry
            });
        }
    }
}
