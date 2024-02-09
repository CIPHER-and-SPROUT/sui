// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
import { Button } from "@radix-ui/themes";
import { ReactNode } from "react";
import { Loading } from "./Loading";

export function InfiniteScrollArea({
  children,
  loadMore,
  loading = false,
  hasNextPage,
  gridClasses = "py-12 grid-cols-1 md:grid-cols-2 gap-5",
}: {
  children: ReactNode | ReactNode[];
  loadMore: () => void;
  loading: boolean;
  hasNextPage: boolean;
  gridClasses?: string;
}) {
  if (!children || (Array.isArray(children) && children.length === 0))
    return <div className="p-3">No results found.</div>;
  return (
    <>
      <div className={`grid ${gridClasses}`}>{children}</div>

      <div className="col-span-2 text-center">
        {loading && <Loading />}

        {hasNextPage && !loading && (
          <Button
            color="gray"
            className="cursor-pointer"
            onClick={loadMore}
            disabled={!hasNextPage || loading}
          >
            Load more...
          </Button>
        )}
      </div>
    </>
  );
}
