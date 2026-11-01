export interface LyricReport {
  epoch: string;
  id: string;
  revision: string;
  title: string;
  options: { key: string; label: string; value: number }[];
  selected: string | null;
  content: string;
  phase: 'draft' | 'rejected' | 'sending' | 'uncertain' | 'sent' | 'abandoned';
  issue: 'not_sent' | 'rejected' | 'unknown' | null;
}
export interface LyricReportClient {
  open(epoch: string, playId: string): Promise<LyricReport>;
  read(epoch: string, id: string): Promise<LyricReport>;
  types(epoch: string, id: string, revision: string): Promise<LyricReport>;
  submit(
    epoch: string,
    id: string,
    revision: string,
    key: string,
    content: string,
  ): Promise<LyricReport>;
  abandon(epoch: string, id: string, revision: string): Promise<LyricReport>;
}
