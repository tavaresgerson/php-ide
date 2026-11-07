<?php

namespace App\Tests;

use App\Util;
use PHPUnit\Framework\TestCase;

final class UtilTest extends TestCase
{
    public function testDobra(): void
    {
        self::assertSame(6, Util::dobra(3));
    }

    public function testIgnorado(): void
    {
        self::markTestSkipped('pulado de propósito');
    }
}
