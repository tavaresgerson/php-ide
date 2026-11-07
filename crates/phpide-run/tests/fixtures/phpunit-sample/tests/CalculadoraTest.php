<?php

namespace App\Tests;

use App\Calculadora;
use PHPUnit\Framework\TestCase;

final class CalculadoraTest extends TestCase
{
    public function testSoma(): void
    {
        $calc = new Calculadora();
        self::assertSame(4, $calc->soma(2, 2));
    }

    /** @dataProvider provedorDivisao */
    public function testDivisao(int $a, int $b, float $esperado): void
    {
        $calc = new Calculadora();
        self::assertSame($esperado, $calc->divisao($a, $b));
    }

    public static function provedorDivisao(): array
    {
        return [
            'positivos' => [10, 2, 5.0],
            'negativos' => [-9, 3, -3.0],
        ];
    }

    public function testDivisaoPorZero(): void
    {
        $calc = new Calculadora();
        $this->expectException(\InvalidArgumentException::class);
        $calc->divisao(1, 0);
    }

    public function testFalhaProposital(): void
    {
        $calc = new Calculadora();
        self::assertSame(99, $calc->soma(1, 1), 'soma deveria falhar para a árvore mostrar o nó');
    }
}
