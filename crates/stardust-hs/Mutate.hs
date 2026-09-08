{-# LANGUAGE ForeignFunctionInterface #-}

module Mutate where

import Data.Word (Word8)
import Foreign.C.Types (CSize(..))
import Foreign.Ptr (Ptr)
import Foreign.Storable (peekByteOff, pokeByteOff)

-- Wrapping +1 per byte over a borrowed region span. n == 0 is a no-op.
mutate_hs :: Ptr Word8 -> CSize -> IO ()
mutate_hs p n
  | n == 0 = return ()
  | otherwise = go 0
  where
    len = fromIntegral n :: Int
    go i
      | i >= len = return ()
      | otherwise = do
          b <- peekByteOff p i :: IO Word8
          pokeByteOff p i (b + 1)
          go (i + 1)

foreign export ccall "mutate_hs" mutate_hs :: Ptr Word8 -> CSize -> IO ()
