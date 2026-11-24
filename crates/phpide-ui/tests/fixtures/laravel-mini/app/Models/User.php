<?php

declare(strict_types=1);

namespace App\Models;

/**
 * A registered user of the application.
 */
class User
{
    public function __construct(
        public int $id,
        public string $firstName,
        public string $lastName,
        public string $email,
    ) {
    }

    /**
     * Finds a user by its primary key.
     */
    public static function find(int $id): ?User
    {
        return $id > 0 ? new User($id, 'Ada', 'Lovelace', 'ada@example.com') : null;
    }

    /**
     * Every user.
     *
     * @return list<User>
     */
    public static function all(): array
    {
        return [self::find(1)];
    }

    /**
     * First and last name, separated by a space.
     */
    public function fullName(): string
    {
        return $this->firstName . ' ' . $this->lastName;
    }

    public function isAdmin(): bool
    {
        return str_ends_with($this->email, '@example.com');
    }
}
