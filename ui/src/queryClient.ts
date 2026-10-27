import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Library data is local; keep it fresh until a mutation invalidates it.
      staleTime: Infinity,
      refetchOnWindowFocus: false,
      retry: false,
    },
  },
});
