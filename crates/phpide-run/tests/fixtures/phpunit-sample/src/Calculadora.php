<?php

namespace App;

final class Calculadora
{
    public function soma(int $a, int $b): int
    {
        return $a + $b;
    }

    public function divisao(int $a, int $b): float
    {
        if ($b === 0) {
            throw new \InvalidArgumentException('divisão por zero');
        }

        return $a / $b;
    }
}
