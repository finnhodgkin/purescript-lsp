module Broken where

import Prelude

-- Missing import for Effect.Console
-- Unknown function 'lg'
main :: Effect Unit  
main = do
  lg "test"
  unknownFunc 123
