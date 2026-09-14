/** Result of a smart pull operation (fetch + intelligent merge). */
export type SmartPullResult =
  | { status: "upToDate" }
  | { status: "success"; commitOid: string }
  | {
      status: "conflict";
      files: string[];
      /** HEAD before the merge (abort target). */
      baseOid: string | null;
    };
