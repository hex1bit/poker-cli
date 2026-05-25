//! 蒙特卡洛 equity 基准。

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use poker::card::{Card, Rank, Suit};
use poker::equity::mc_equity;
use rand::SeedableRng;

fn bench_equity_flop_6way(c: &mut Criterion) {
    let hero = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::King, Suit::Hearts),
    ];
    let board = [
        Card::new(Rank::Queen, Suit::Hearts),
        Card::new(Rank::Seven, Suit::Diamonds),
        Card::new(Rank::Two, Suit::Clubs),
    ];
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);

    c.bench_function("mc_equity flop 6-way 800 iters", |b| {
        b.iter(|| {
            let _ = black_box(mc_equity(hero, &board, 5, 800, &mut rng));
        })
    });
}

fn bench_equity_river_3way(c: &mut Criterion) {
    let hero = [
        Card::new(Rank::Ace, Suit::Hearts),
        Card::new(Rank::Ace, Suit::Spades),
    ];
    let board = [
        Card::new(Rank::King, Suit::Hearts),
        Card::new(Rank::Queen, Suit::Diamonds),
        Card::new(Rank::Five, Suit::Clubs),
        Card::new(Rank::Two, Suit::Spades),
        Card::new(Rank::Nine, Suit::Hearts),
    ];
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);

    c.bench_function("mc_equity river 3-way 800 iters", |b| {
        b.iter(|| {
            let _ = black_box(mc_equity(hero, &board, 2, 800, &mut rng));
        })
    });
}

criterion_group!(benches, bench_equity_flop_6way, bench_equity_river_3way);
criterion_main!(benches);
