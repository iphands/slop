export interface Player {
  clientNum: number;
  score: number;
  address: string;
  name: string;
  ping: number;
}

export interface PlayerList {
  players: Player[];
}
