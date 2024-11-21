# Runtime

This is where the Plaid runtime code lives along with the STL used by modules. The reason the STL lives in here is because there are shared structures between the `plaid` and `plaid-stl` codebases and having the `plaid-stl` here allows it to be pulled in by modules, but the reverse was not true last time I tried. I also feel these have more in common that the modules do with the STL.