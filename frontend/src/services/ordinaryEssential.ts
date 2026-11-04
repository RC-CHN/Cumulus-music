import type { AccountData } from './contracts';

export type EssentialCategory = 0 | 1 | 2 | 3 | 4;
export type EssentialRail = 'playlists' | 'songs' | 'books' | 'voices';

export interface EssentialCard {
  id: string;
  title: string;
  subtitle: string | null;
  coverUrl: string | null;
  radioId: string | null;
  history: boolean;
}

export interface EssentialBooks {
  title: string;
  cards: EssentialCard[];
}

export interface OrdinaryEssentialContent {
  playlists: EssentialCard[];
  songs: EssentialCard[];
  books: EssentialBooks | null;
  voices: EssentialCard[];
}

export interface OrdinaryEssentialView {
  content: OrdinaryEssentialContent;
  failedBlocks: number;
}

export interface OrdinaryEssentialClient {
  ordinaryEssential(
    epoch: string,
    category: EssentialCategory,
  ): Promise<AccountData<OrdinaryEssentialView>>;
  ordinaryEssentialBooks(
    epoch: string,
    category: EssentialCategory,
  ): Promise<AccountData<EssentialBooks | null>>;
}
