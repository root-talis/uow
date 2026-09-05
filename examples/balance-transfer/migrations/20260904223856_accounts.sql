create table accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    balance INTEGER NOT NULL
);

insert into accounts (id, balance) values (1, 100000), (2, 200000);
