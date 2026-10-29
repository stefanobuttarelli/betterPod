wormhole-william v1.0.8 (Peter Sanford, MIT: see LICENSE), trimmed for
betterPod: only the packages the library needs (wormhole, rendezvous,
internal, wordlist, version), without the command-line tool, its
dependencies and the tests. The code is unchanged but for two additions
for Share, marked "betterPod" in the code:

- `WithOfferInfo` puts the app's own JSON in a directory offer (the
  `info` field, which other clients ignore), read by the receiver as
  `IncomingMessage.Info` before it accepts;
- `IncomingMessage.RejectWith(reason)` rejects with that reason instead
  of "transfer rejected"; the sender gets it as "TransferError: reason".
