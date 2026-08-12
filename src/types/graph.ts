export interface CommitInfo {
  oid: string;
  shortOid: string;
  message: string;
  author: string;
  email: string;
  time: string;
  parents: string[];
  refs: string[];
}

export interface BranchInfo {
  name: string;
  isRemote: boolean;
  isCurrent: boolean;
  lastCommitOid: string;
  lastCommitMessage: string;
}
