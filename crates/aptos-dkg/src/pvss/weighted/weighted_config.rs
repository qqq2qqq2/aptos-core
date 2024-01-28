// Copyright © Aptos Foundation

use crate::{
    algebra::evaluation_domain::{BatchEvaluationDomain, EvaluationDomain},
    pvss::{traits, traits::SecretSharingConfig, Player, ThresholdConfig},
};
use anyhow::anyhow;
use more_asserts::assert_lt;
use rand::Rng;
use rand_core::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// Encodes the *threshold configuration* for a *weighted* PVSS: i.e., the minimum weight $w$ and
/// the total weight $W$ such that any subset of players with weight $\ge w$ can reconstruct a
/// dealt secret given a PVSS transcript.
#[allow(non_snake_case)]
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct WeightedConfig {
    /// A weighted config is a $w$-out-of-$W$ threshold config, where $w$ is the minimum weight
    /// needed to reconstruct the secret and $W$ is the total weight.
    tc: ThresholdConfig,
    /// The total number of players in the protocol.
    num_players: usize,
    /// Each player's weight
    weight: Vec<usize>,
    /// Player's starting index `a` in a vector of all `W` shares, such that this player owns shares
    /// `W[a, a + weight[player])`. Useful during weighted secret reconstruction.
    starting_index: Vec<usize>,
    /// The maximum weight of any player.
    max_player_weight: usize,
}

impl WeightedConfig {
    #[allow(non_snake_case)]
    /// Initializes a weighted secret sharing configuration with threshold weight `threshold_weight`
    /// and the $i$th player's weight stored in `weight[i]`.
    pub fn new(threshold_weight: usize, weights: Vec<usize>) -> anyhow::Result<Self> {
        if threshold_weight == 0 {
            return Err(anyhow!(
                "expected the minimum reconstruction weight to be > 0"
            ));
        }

        if weights.is_empty() {
            return Err(anyhow!("expected a non-empty vector of player weights"));
        }
        let max_player_weight = *weights.iter().max().unwrap();

        for (idx, w) in weights.iter().enumerate() {
            if *w == 0 {
                return Err(anyhow!("expected player at index {idx} to have weight > 0"));
            }
        }

        let n = weights.len();
        let W = weights.iter().sum();

        // e.g., Suppose the weights for players 0, 1 and 2 are [2, 4, 3]
        // Then, our PVSS transcript implementation will store a vector of 2 + 4 + 3 = 9 shares,
        // such that:
        //  - Player 0 will own the shares at indices [0..2), i.e.,starting index 0
        //  - Player 1 will own the shares at indices [2..2 + 4) = [2..6), i.e.,starting index 2
        //  - Player 2 will own the shares at indices [6, 6 + 3) = [6..9), i.e., starting index 6
        let mut starting_index = Vec::with_capacity(weights.len());
        starting_index.push(0);

        for w in weights.iter().take(n - 1) {
            starting_index.push(starting_index.last().unwrap() + w);
        }

        let tc = ThresholdConfig::new(threshold_weight, W)?;
        Ok(WeightedConfig {
            tc,
            num_players: n,
            weight: weights,
            starting_index,
            max_player_weight,
        })
    }

    pub fn get_max_player_weight(&self) -> usize {
        self.max_player_weight
    }

    pub fn get_threshold_config(&self) -> &ThresholdConfig {
        &self.tc
    }

    pub fn get_threshold_weight(&self) -> usize {
        self.tc.t
    }

    pub fn get_total_weight(&self) -> usize {
        self.tc.n
    }

    pub fn get_player_weight(&self, player: &Player) -> usize {
        self.weight[player.id]
    }

    pub fn get_player_starting_index(&self, player: &Player) -> usize {
        self.starting_index[player.id]
    }

    /// In an unweighted secret sharing scheme, each player has one share. We can weigh such a scheme
    /// by splitting a player into as many "virtual" players as that player's weight, assigning one
    /// share per "virtual player."
    ///
    /// This function returns the "virtual" player associated with the $i$th sub-share of this player.
    pub fn get_virtual_player(&self, player: &Player, j: usize) -> Player {
        // println!("WeightedConfig::get_virtual_player({player}, {i})");
        assert_lt!(j, self.weight[player.id]);

        let id = self.get_share_index(player.id, j).unwrap();

        Player { id }
    }

    pub fn get_all_virtual_players(&self, player: &Player) -> Vec<Player> {
        let w = self.get_player_weight(player);

        (0..w)
            .map(|i| self.get_virtual_player(player, i))
            .collect::<Vec<Player>>()
    }

    /// `i` is the player's index, from 0 to `self.tc.n`
    /// `j` is the player's share #, from 0 to `self.weight[i]`
    ///
    /// Returns the index of this player's share in the vector of shares, or None if out of bounds.
    pub fn get_share_index(&self, i: usize, j: usize) -> Option<usize> {
        assert_lt!(i, self.tc.n);
        if j < self.weight[i] {
            Some(self.starting_index[i] + j)
        } else {
            None
        }
    }

    pub fn get_batch_evaluation_domain(&self) -> &BatchEvaluationDomain {
        &self.tc.get_batch_evaluation_domain()
    }

    pub fn get_evaluation_domain(&self) -> &EvaluationDomain {
        &self.tc.get_evaluation_domain()
    }

    pub fn get_best_case_eligible_subset_of_players<R: RngCore + CryptoRng>(
        &self,
        _rng: &mut R,
    ) -> Vec<Player> {
        let mut player_and_weights = self.sort_players_by_weight();

        self.pop_eligible_subset(&mut player_and_weights)
    }

    pub fn get_worst_case_eligible_subset_of_players<R: RngCore + CryptoRng>(
        &self,
        _rng: &mut R,
    ) -> Vec<Player> {
        let mut player_and_weights = self.sort_players_by_weight();

        player_and_weights.reverse();

        self.pop_eligible_subset(&mut player_and_weights)
    }

    fn sort_players_by_weight(&self) -> Vec<(usize, usize)> {
        // the set of remaining players that we are picking a "capable" subset from
        let mut player_and_weights = self
            .weight
            .iter()
            .enumerate()
            .map(|(i, w)| (i, *w))
            .collect::<Vec<(usize, usize)>>();

        player_and_weights.sort_by(|a, b| a.1.cmp(&b.1));
        player_and_weights
    }

    fn pop_eligible_subset(&self, player_and_weights: &mut Vec<(usize, usize)>) -> Vec<Player> {
        let mut picked_players = vec![];

        let mut current_weight = 0;
        while current_weight < self.tc.t {
            let (player_idx, weight) = player_and_weights.pop().unwrap();

            picked_players.push(self.get_player(player_idx));

            // rinse and repeat until the picked players jointly have enough weight
            current_weight += weight;
        }

        picked_players
    }
}

impl Display for WeightedConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "weighted/{}-out-of-{}/{}-players",
            self.tc.t, self.tc.n, self.num_players
        )
    }
}

impl traits::SecretSharingConfig for WeightedConfig {
    /// For testing only.
    fn get_random_player<R>(&self, rng: &mut R) -> Player
    where
        R: RngCore + CryptoRng,
    {
        Player {
            id: rng.gen_range(0, self.get_total_num_players()),
        }
    }

    fn get_random_eligible_subset_of_players<R>(&self, rng: &mut R) -> Vec<Player>
    where
        R: RngCore,
    {
        // the randomly-picked "capable" subset of players who can reconstruct the secret
        let mut picked_players = vec![];
        // the set of remaining players that we are picking a "capable" subset from
        let mut player_and_weights = self
            .weight
            .iter()
            .enumerate()
            .map(|(i, w)| (i, *w))
            .collect::<Vec<(usize, usize)>>();
        let mut current_weight = 0;

        while current_weight < self.tc.t {
            // pick a random player, and move it to the picked set
            let idx = rng.gen_range(0, player_and_weights.len());
            let (player_id, weight) = player_and_weights[idx];
            picked_players.push(self.get_player(player_id));

            // efficiently remove the picked player from the set of remaining players
            let len = player_and_weights.len();
            if len > 1 {
                player_and_weights.swap(idx, len - 1);
                player_and_weights.pop();
            }

            // rinse and repeat until the picked players jointly have enough weight
            current_weight += weight;
        }

        // println!();
        // println!(
        //     "Returned random capable subset {{ {} }}",
        //     vec_to_str!(picked_players)
        // );
        picked_players
    }

    fn get_total_num_players(&self) -> usize {
        self.num_players
    }

    fn get_total_num_shares(&self) -> usize {
        self.tc.n
    }
}

#[cfg(test)]
mod test {
    use crate::pvss::{traits::SecretSharingConfig, WeightedConfig};

    #[test]
    fn bvt() {
        // 1-out-of-1 weighted
        let wc = WeightedConfig::new(1, vec![1]).unwrap();
        assert_eq!(wc.starting_index.len(), 1);
        assert_eq!(wc.starting_index[0], 0);
        assert_eq!(wc.get_virtual_player(&wc.get_player(0), 0).id, 0);

        // 1-out-of-2, weights 2
        let wc = WeightedConfig::new(1, vec![2]).unwrap();
        assert_eq!(wc.starting_index.len(), 1);
        assert_eq!(wc.starting_index[0], 0);
        assert_eq!(wc.get_virtual_player(&wc.get_player(0), 0).id, 0);
        assert_eq!(wc.get_virtual_player(&wc.get_player(0), 1).id, 1);

        // 1-out-of-2, weights 1, 1
        let wc = WeightedConfig::new(1, vec![1, 1]).unwrap();
        assert_eq!(wc.starting_index.len(), 2);
        assert_eq!(wc.starting_index[0], 0);
        assert_eq!(wc.starting_index[1], 1);
        assert_eq!(wc.get_virtual_player(&wc.get_player(0), 0).id, 0);
        assert_eq!(wc.get_virtual_player(&wc.get_player(1), 0).id, 1);

        // 2-out-of-2, weights 1, 1
        let _wc = WeightedConfig::new(1, vec![1, 1]).unwrap();
        assert_eq!(wc.starting_index.len(), 2);
        assert_eq!(wc.starting_index[0], 0);
        assert_eq!(wc.starting_index[1], 1);
        assert_eq!(wc.get_virtual_player(&wc.get_player(0), 0).id, 0);
        assert_eq!(wc.get_virtual_player(&wc.get_player(1), 0).id, 1);
    }
}